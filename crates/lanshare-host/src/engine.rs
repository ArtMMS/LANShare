//! Motor do Host: abre a sala, aceita Clients, transmite a tela do Host e coordena
//! as transmissões de todos (quem transmite e os ingressos para assistir).
//!
//! Este módulo NÃO imprime nada e NÃO lê o teclado. Quem usa (o terminal ou a janela)
//! manda comandos pelos métodos de `HostEngine` e recebe o que acontece pelos eventos.
//!
//! v2.0: o vídeo não passa pela conexão da sala. Quem transmite (o Host ou um Client) tem o
//! próprio StreamServer; o Host só diz onde assistir e entrega os ingressos.

use crate::discovery::{self, RoomSettings};
use crate::events::{EventSender, HostEvent, LocalAddress, UserEntry};
use crate::kick::Kicker;
use crate::registry::{ClientInfo, Registry};
use crate::streams::{StreamDirectory, StreamSpot};
use lanshare_core::i18n::tr;
use lanshare_core::protocol::{
    new_watch_ticket, read_message, write_message, Message, SfxFrame, UserInfo, DEFAULT_PORT,
    HOST_USER_ID, MAX_AVATAR_LEN, PROTOCOL_VERSION, REJECT_PASSWORD_TIMEOUT,
    REJECT_PROTOCOL_MISMATCH, REJECT_ROOM_FULL, REJECT_WRONG_PASSWORD, WATCH_BUSY_STREAMING,
    WATCH_NOT_STREAMING, WATCH_SELF,
};
use lanshare_core::settings;
use lanshare_core::sfx::{self, SfxCooldown, SfxInbox};
use lanshare_net::{run_connection, ConnectionEvent};
use lanshare_stream::{CaptureTarget, PreviewSlot, StreamHandle, StreamServer, StreamSettings};
use serde::Deserialize;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self as std_mpsc, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::sync::{oneshot, watch};

/// Tentativas de senha por conexão.
const MAX_PASSWORD_ATTEMPTS: u32 = 3;

/// Tempo que o Client tem para responder com a senha.
const PASSWORD_TIMEOUT: Duration = Duration::from_secs(30);

/// Pausa depois de uma senha errada (atrapalha tentativas em sequência).
const WRONG_PASSWORD_DELAY: Duration = Duration::from_secs(1);

/// Tempo para o aviso de saída (Bye) chegar aos Clients antes de fechar tudo.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// Tamanho máximo do nome da sala (em caracteres).
const MAX_ROOM_NAME_LEN: usize = 32;

/// De quanto em quanto tempo o vigia confere se a transmissão do Host caiu sozinha.
const STREAM_WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// Opções da sala, escolhidas pelo Host antes de abrir.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HostConfig {
    /// Senha da sala. None (ou vazia) = sala sem senha.
    pub password: Option<String>,
    /// Limite de Clients. None = sem limite.
    pub max_clients: Option<usize>,
    /// Nome da sala na lista de salas dos Clients. None = nome do computador.
    pub room_name: Option<String>,
}

/// A transmissão do próprio Host: a captura/compressão e a porta onde os espectadores assistem.
struct HostStream {
    handle: StreamHandle,
    server: StreamServer,
}

impl HostStream {
    /// Fecha a porta (avisa quem assiste) e para a captura.
    fn stop(self) {
        self.server.stop();
        self.handle.stop();
    }
}

/// Dados compartilhados por todas as conexões.
#[derive(Clone)]
struct Shared {
    registry: Registry,
    kicker: Kicker,
    password: Option<Arc<String>>,
    events: EventSender,
    /// Nome de usuário do Host, mostrado na lista de usuários dos Clients.
    host_name: Arc<String>,
    /// Avatar do Host (PNG em base64). None = sem avatar.
    host_avatar: Arc<Mutex<Option<String>>>,
    /// Efeitos sonoros que os Clients tocaram, esperando a interface do Host buscar.
    sfx_inbox: SfxInbox,
    /// Quem está transmitindo na sala e onde assistir.
    streams: StreamDirectory,
    /// A transmissão do Host (None = não está transmitindo).
    host_stream: Arc<Mutex<Option<HostStream>>>,
}

impl Shared {
    fn lock_host_stream(&self) -> MutexGuard<'_, Option<HostStream>> {
        self.host_stream.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Alguém começou a transmitir: anota no catálogo e avisa a sala (aparece o ícone de tela).
    fn announce_stream(&self, id: u64, spot: StreamSpot) {
        self.registry.broadcast(&spot.message(id), Some(id));
        self.streams.set(id, spot);
        self.events.emit(HostEvent::UserStreaming { id });
    }

    /// Alguém parou de transmitir (ou saiu da sala): tira do catálogo e avisa a sala.
    fn end_stream(&self, id: u64) {
        if !self.streams.remove(id) {
            return;
        }
        self.registry
            .broadcast(&Message::UserStreamStopped { user_id: id }, None);
        self.events.emit(HostEvent::UserStreamStopped { id });
    }

    /// `viewer` quer assistir `target`: emite o ingresso, avisa quem transmite e responde.
    fn watch_request(&self, viewer: u64, target: u64) {
        let deny = |code: &str| {
            self.registry.send_to(
                viewer,
                Message::WatchDenied {
                    user_id: target,
                    code: code.to_string(),
                },
            );
        };

        if viewer == target {
            return deny(WATCH_SELF);
        }
        // Na v2.0 quem transmite não assiste outra pessoa ao mesmo tempo
        if self.streams.contains(viewer) {
            return deny(WATCH_BUSY_STREAMING);
        }
        let Some(spot) = self.streams.get(target) else {
            return deny(WATCH_NOT_STREAMING);
        };

        let ticket = new_watch_ticket();
        let allowed = if target == HOST_USER_ID {
            self.lock_host_stream()
                .as_ref()
                .is_some_and(|stream| stream.server.allow_viewer(viewer, ticket.clone()))
        } else {
            self.registry.send_to(
                target,
                Message::ViewerAllowed {
                    viewer_id: viewer,
                    ticket: ticket.clone(),
                },
            )
        };
        if !allowed {
            return deny(WATCH_NOT_STREAMING);
        }

        self.registry.send_to(
            viewer,
            Message::WatchGranted {
                user_id: target,
                ip: spot.ip,
                port: spot.port,
                ticket,
            },
        );
    }

    /// `viewer` não pode mais assistir ninguém (saiu, foi expulso ou começou a transmitir).
    fn revoke_viewer_everywhere(&self, viewer: u64) {
        if let Some(stream) = self.lock_host_stream().as_ref() {
            stream.server.revoke_viewer(viewer);
        }

        for (streamer, _) in self.streams.all() {
            if streamer != HOST_USER_ID && streamer != viewer {
                self.registry
                    .send_to(streamer, Message::ViewerRevoked { viewer_id: viewer });
            }
        }
    }
}

/// A sala aberta. Tem o seu próprio runtime do tokio, então pode ser usada de qualquer thread.
pub struct HostEngine {
    runtime: Runtime,
    registry: Registry,
    kicker: Kicker,
    events: EventSender,
    event_rx: Mutex<Receiver<HostEvent>>,
    shutdown_tx: watch::Sender<bool>,
    shared: Shared,
    /// Miniatura do que o Host está transmitindo (preview do Host).
    preview: PreviewSlot,
    /// Avatar do Host, enviado a quem entra na sala.
    host_avatar: Arc<Mutex<Option<String>>>,
    /// Efeitos sonoros que os Clients tocaram (a interface busca com take_sfx).
    sfx_inbox: SfxInbox,
    /// Anti-spam dos sons do próprio Host.
    sfx_cooldown: Mutex<SfxCooldown>,
    /// Botão 🔊 do seletor: a transmissão leva som? (vale para a próxima e para a atual)
    stream_audio: AtomicBool,
}

impl HostEngine {
    /// Abre a sala: escuta Clients na rede e responde à descoberta automática.
    /// Falha, por exemplo, se a porta já estiver em uso.
    pub fn start(config: HostConfig) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("lanshare-host")
            .build()?;

        let address = format!("0.0.0.0:{DEFAULT_PORT}");
        let listener = runtime.block_on(TcpListener::bind(&address))?;

        let (event_tx, event_rx) = std_mpsc::channel();
        let events = EventSender::new(event_tx);

        let password = config
            .password
            .filter(|typed| !typed.trim().is_empty())
            .map(Arc::new);
        let registry = Registry::new(config.max_clients);
        let kicker = Kicker::new();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let host_avatar: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let sfx_inbox = SfxInbox::default();

        let shared = Shared {
            registry: registry.clone(),
            kicker: kicker.clone(),
            password: password.clone(),
            events: events.clone(),
            host_name: Arc::new(host_display_name()),
            host_avatar: host_avatar.clone(),
            sfx_inbox: sfx_inbox.clone(),
            streams: StreamDirectory::default(),
            host_stream: Arc::new(Mutex::new(None)),
        };

        events.emit(HostEvent::Started {
            address,
            has_password: password.is_some(),
            max_clients: config.max_clients,
            local_addresses: local_addresses(&events),
        });

        // Responde aos Clients que procuram salas na rede (UDP)
        runtime.spawn(discovery::run_responder(
            registry.clone(),
            events.clone(),
            RoomSettings {
                name: room_name(config.room_name),
                has_password: password.is_some(),
                max_clients: config.max_clients,
            },
        ));

        // Avisa a sala se a transmissão do Host cair sozinha (por exemplo, por erro de captura)
        runtime.spawn(watch_stream(shared.clone(), shutdown_rx.clone()));

        // Aceita os Clients que chegarem
        runtime.spawn(accept_loop(listener, shutdown_rx, shared.clone()));

        Ok(Self {
            runtime,
            registry,
            kicker,
            events,
            event_rx: Mutex::new(event_rx),
            shutdown_tx,
            shared,
            preview: PreviewSlot::default(),
            host_avatar,
            sfx_inbox,
            sfx_cooldown: Mutex::new(SfxCooldown::default()),
            stream_audio: AtomicBool::new(true),
        })
    }

    /// Próximo evento que ainda não foi lido (None = não há nenhum no momento).
    pub fn try_next_event(&self) -> Option<HostEvent> {
        self.event_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv()
            .ok()
    }

    /// Quem está conectado agora, ordenado por ID.
    pub fn users(&self) -> Vec<UserEntry> {
        self.registry
            .list()
            .into_iter()
            .map(|c| UserEntry {
                id: c.id,
                name: c.name,
                addr: c.addr.to_string(),
                connected_secs: c.connected_at.elapsed().as_secs(),
            })
            .collect()
    }

    /// Define (ou apaga, com None) o avatar do Host e avisa todos os Clients.
    pub fn set_avatar(&self, png_base64: Option<String>) -> Result<(), String> {
        if png_base64.as_ref().is_some_and(|a| a.len() > MAX_AVATAR_LEN) {
            return Err(tr(
                "O avatar é grande demais para enviar.",
                "The avatar is too large to send.",
                "El avatar es demasiado grande para enviar.",
            )
            .to_string());
        }

        *self.host_avatar.lock().unwrap_or_else(|e| e.into_inner()) = png_base64.clone();

        let notice = Message::UserAvatar {
            id: HOST_USER_ID,
            png_base64: png_base64.unwrap_or_default(),
        };
        self.registry.broadcast(&notice, None);
        Ok(())
    }

    /// Toca um efeito sonoro do Host para todos os Clients (a interface já tocou ele aqui).
    /// `packets`: o som já comprimido com `sfx::encode` (comprima antes, fora de qualquer lock).
    /// Erro = o som anterior do Host ainda está tocando (anti-spam: um som por vez).
    pub fn play_sfx(&self, name: &str, packets: Vec<Vec<u8>>) -> Result<(), String> {
        let allowed = self
            .sfx_cooldown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_start(sfx::play_time(packets.len()), sfx::COOLDOWN_SLACK);
        if !allowed {
            return Err(sfx::too_soon_message());
        }

        let frame = SfxFrame {
            user_id: HOST_USER_ID,
            name: sfx::clean_name(name),
            packets: Arc::new(packets),
        };
        self.registry.broadcast(&Message::Sfx(frame), None);
        Ok(())
    }

    /// Busca (e descomprime) um efeito sonoro avisado no evento "sfx_played".
    /// None = já foi buscado, foi descartado ou veio inválido.
    pub fn take_sfx(&self, sfx_id: u64) -> Option<Vec<f32>> {
        self.sfx_inbox.take(sfx_id)
    }

    /// Remove um Client: avisa ele, avisa os outros e encerra a conexão dele.
    /// Devolve o nome de quem foi removido, ou o motivo de não ter dado certo.
    pub fn kick(&self, id: u64) -> Result<String, String> {
        let Some(target) = self.registry.list().into_iter().find(|c| c.id == id) else {
            return Err(format!(
                "{} #{id}.",
                tr(
                    "Não existe usuário com o id",
                    "There is no user with id",
                    "No existe ningún usuario con el id"
                )
            ));
        };

        // O Kicker usa tokio::spawn, que precisa estar dentro do runtime
        let _guard = self.runtime.enter();
        if !self.kicker.kick(id, "Voce foi removido pelo Host") {
            return Err(format!(
                "{} (#{id}) {}",
                target.name,
                tr("já está saindo.", "is already leaving.", "ya está saliendo.")
            ));
        }

        // Avisa todos os outros (o expulso já recebeu o aviso dele)
        let notice = Message::UserKicked {
            user: UserInfo {
                id,
                name: target.name.clone(),
            },
        };
        self.registry.broadcast(&notice, Some(id));

        Ok(target.name)
    }

    /// Começa a transmitir o monitor principal.
    pub fn start_stream(&self, settings: StreamSettings) -> Result<(), String> {
        self.start_stream_on(settings, CaptureTarget::PrimaryMonitor)
    }

    /// Começa a transmitir um monitor ou uma janela (com ou sem som, conforme o último
    /// `set_stream_audio`) e avisa a sala. Só quem pedir ingresso recebe a imagem.
    pub fn start_stream_on(
        &self,
        settings: StreamSettings,
        target: CaptureTarget,
    ) -> Result<(), String> {
        if !matches!(settings.height, 720 | 1080) || !matches!(settings.fps, 15 | 30) {
            return Err(tr(
                "Resolução ou FPS inválidos (use 720 ou 1080, e 15 ou 30).",
                "Invalid resolution or FPS (use 720 or 1080, and 15 or 30).",
                "Resolución o FPS no válidos (usa 720 o 1080, y 15 o 30).",
            )
            .to_string());
        }

        let mut slot = self.shared.lock_host_stream();
        if slot.as_ref().is_some_and(|s| s.handle.is_running()) {
            return Err(tr(
                "A tela já está sendo compartilhada.",
                "The screen is already being shared.",
                "La pantalla ya se está compartiendo.",
            )
            .to_string());
        }

        // Uma transmissão anterior que caiu sozinha (o vigia ainda não limpou): fecha a porta dela
        if let Some(old) = slot.take() {
            old.stop();
        }

        let server = StreamServer::start(self.runtime.handle(), self.events.clone().into())
            .map_err(|erro| {
                format!(
                    "{}: {erro}",
                    tr(
                        "Não foi possível abrir a porta da transmissão",
                        "Could not open the stream port",
                        "No se pudo abrir el puerto de la transmisión"
                    )
                )
            })?;
        let port = server.port();

        let handle = StreamHandle::start_on(
            server.clone(),
            settings,
            self.events.clone(),
            self.preview.clone(),
            target,
            self.stream_audio.load(Ordering::Relaxed),
        );
        *slot = Some(HostStream { handle, server });
        drop(slot);

        self.shared
            .announce_stream(HOST_USER_ID, StreamSpot { ip: None, port });
        Ok(())
    }

    /// Liga ou desliga o som da transmissão (botão 🔊 do seletor).
    /// Vale para a próxima transmissão e, se já estiver transmitindo, muda na hora.
    pub fn set_stream_audio(&self, enabled: bool) {
        self.stream_audio.store(enabled, Ordering::Relaxed);

        let slot = self.shared.lock_host_stream();
        if let Some(stream) = slot.as_ref().filter(|s| s.handle.is_running()) {
            stream.handle.set_audio_enabled(enabled);
        }
    }

    /// A transmissão leva som?
    pub fn stream_audio(&self) -> bool {
        self.stream_audio.load(Ordering::Relaxed)
    }

    /// Troca o monitor ou a janela transmitida sem parar a transmissão.
    pub fn switch_source(&self, target: CaptureTarget) -> Result<(), String> {
        let slot = self.shared.lock_host_stream();
        match slot.as_ref() {
            Some(stream) if stream.handle.is_running() => stream.handle.switch_source(target),
            _ => Err(tr(
                "Não há transmissão em andamento.",
                "There is no stream in progress.",
                "No hay ninguna transmisión en curso.",
            )
            .to_string()),
        }
    }

    /// Para a transmissão do Host e avisa a sala. Devolve false se não havia transmissão.
    pub fn stop_stream(&self) -> bool {
        // Tira da caixa antes de esperar a thread terminar (para não segurar o lock)
        let stream = self.shared.lock_host_stream().take();

        match stream {
            Some(running) => {
                running.stop();
                self.shared.end_stream(HOST_USER_ID);
                true
            }
            None => false,
        }
    }

    /// Copia a miniatura da transmissão (BGRA, 4 bytes por pixel) para `out`.
    /// Devolve (largura, altura), ou None se não há miniatura ou `out` é pequeno.
    pub fn preview_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        self.preview.read_into(out)
    }

    /// O Host está transmitindo? (false também se a transmissão caiu por erro)
    pub fn is_streaming(&self) -> bool {
        self.shared
            .lock_host_stream()
            .as_ref()
            .is_some_and(|s| s.handle.is_running())
    }

    /// Acesso ao runtime do tokio da sala (por exemplo, para o terminal ouvir o Ctrl+C).
    pub fn runtime_handle(&self) -> Handle {
        self.runtime.handle().clone()
    }

    /// Fecha a sala: para a transmissão, avisa os Clients e encerra tudo.
    /// Devolve os eventos que ainda não tinham sido lidos (inclui o `Stopped`).
    pub fn stop(self) -> Vec<HostEvent> {
        self.stop_stream();
        let _ = self.shutdown_tx.send(true);

        // Dá tempo para o aviso de saída chegar aos Clients
        std::thread::sleep(SHUTDOWN_GRACE);

        self.events.emit(HostEvent::Stopped);

        let mut remaining = Vec::new();
        while let Some(event) = self.try_next_event() {
            remaining.push(event);
        }

        self.runtime.shutdown_timeout(Duration::from_secs(2));
        remaining
    }
}

/// Se a transmissão do Host cair sozinha (por exemplo, erro de captura),
/// fecha a porta dela e avisa a sala, do mesmo jeito que numa parada normal.
async fn watch_stream(shared: Shared, mut shutdown: watch::Receiver<bool>) {
    let mut ticker = tokio::time::interval(STREAM_WATCH_INTERVAL);
    let mut dead_polls = 0u32;

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown.changed() => break,
        }

        let ended = {
            let mut slot = shared.lock_host_stream();
            let dead = slot.as_ref().is_some_and(|s| !s.handle.is_running());
            dead_polls = if dead { dead_polls + 1 } else { 0 };

            // Duas checagens seguidas, para não confundir o início da transmissão com uma queda
            if dead_polls >= 2 {
                slot.take()
            } else {
                None
            }
        };

        if let Some(stream) = ended {
            dead_polls = 0;
            stream.stop();
            shared.end_stream(HOST_USER_ID);
        }
    }
}

/// Aceita Clients até a sala ser fechada.
async fn accept_loop(listener: TcpListener, shutdown: watch::Receiver<bool>, shared: Shared) {
    let mut stop = shutdown.clone();

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, client_addr)) => {
                    // Sem isso o TCP junta pacotes pequenos e atrasa as mensagens
                    let _ = stream.set_nodelay(true);
                    tokio::spawn(handle_client(
                        stream,
                        client_addr,
                        shutdown.clone(),
                        shared.clone(),
                    ));
                }
                Err(erro) => {
                    shared.events.warn(format!("Falha ao aceitar uma conexao: {erro}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },

            _ = stop.changed() => break,
        }
    }
}

/// Cuida de UM Client do começo ao fim.
async fn handle_client(
    mut stream: TcpStream,
    addr: SocketAddr,
    shutdown: watch::Receiver<bool>,
    shared: Shared,
) {
    // Caixa de saída desta conexão: quem quiser falar com este Client põe aqui
    let (out_tx, out_rx) = mpsc::unbounded_channel::<Message>();
    let kick_out_tx = out_tx.clone();
    let intro_tx = out_tx.clone();

    let client = match handshake(&mut stream, addr, &shared, out_tx).await {
        Ok(Some(client)) => client,
        Ok(None) => return, // recusado; o motivo já foi avisado por evento
        Err(erro) => {
            shared
                .events
                .warn(format!("Falha no handshake com {addr}: {erro}"));
            return;
        }
    };
    let (id, name) = (client.id, client.name);

    shared.events.emit(HostEvent::UserJoined {
        id,
        name: name.clone(),
        addr: addr.to_string(),
    });

    // Permite ao Host expulsar este Client (comando kick)
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    shared.kicker.register(id, kick_out_tx, stop_tx);

    // Sinal de encerramento SÓ desta conexão: dispara no kick ou no encerramento do Host
    let (conn_shutdown_tx, conn_shutdown_rx) = watch::channel(false);
    let mut global_shutdown = shutdown.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = global_shutdown.changed() => {},
            _ = stop_rx => {},
            _ = conn_shutdown_tx.closed() => return, // a conexão já acabou sozinha
        }
        let _ = conn_shutdown_tx.send(true);
    });

    // Avisa os outros Clients que alguém entrou
    let joined = Message::UserJoined {
        user: UserInfo {
            id,
            name: name.clone(),
        },
    };
    shared.registry.broadcast(&joined, Some(id));

    // Quem entra recebe os avatares de quem já está na sala (o Host e os outros Clients)...
    let host_avatar = shared
        .host_avatar
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(avatar) = host_avatar {
        let _ = intro_tx.send(Message::UserAvatar {
            id: HOST_USER_ID,
            png_base64: avatar,
        });
    }
    for (other, avatar) in shared.registry.avatars(Some(id)) {
        let _ = intro_tx.send(Message::UserAvatar {
            id: other,
            png_base64: avatar,
        });
    }

    // ...e quem está transmitindo agora (aparece o ícone de tela ao lado do nome)
    for (streamer, spot) in shared.streams.all() {
        let _ = intro_tx.send(spot.message(streamer));
    }
    drop(intro_tx);

    // Guardamos uma cópia para saber, no fim, se o Host inteiro está encerrando
    let shutdown_check = shutdown.clone();

    // O ping de cada Client vira um evento; o avatar dele é guardado e repassado aos outros;
    // o efeito sonoro dele é repassado aos outros e guardado para a interface do Host tocar;
    // as mensagens de transmissão passam pela coordenação (Shared)
    let conn_events = shared.events.clone();
    let conn_registry = shared.registry.clone();
    let conn_inbox = shared.sfx_inbox.clone();
    let conn_shared = shared.clone();
    // Anti-spam deste Client: quem garante é o Host (um Client modificado não consegue burlar)
    let mut sfx_cooldown = SfxCooldown::default();
    let reason = run_connection(stream, conn_shutdown_rx, out_rx, move |event| match event {
        ConnectionEvent::Rtt(rtt) => conn_events.emit(HostEvent::Ping {
            id,
            rtt_ms: rtt.as_millis() as u64,
        }),
        ConnectionEvent::Message(Message::Avatar { png_base64 }) => {
            if png_base64.len() > MAX_AVATAR_LEN {
                conn_events.warn(format!("Avatar do client #{id} ignorado (grande demais)."));
                return;
            }
            let avatar = (!png_base64.is_empty()).then(|| png_base64.clone());
            conn_registry.set_avatar(id, avatar);
            conn_registry.broadcast(
                &Message::UserAvatar {
                    id,
                    png_base64: png_base64.clone(),
                },
                Some(id),
            );
            conn_events.emit(HostEvent::UserAvatar { id, png_base64 });
        }
        ConnectionEvent::Message(Message::Sfx(frame)) => {
            // O som anterior dele ainda está tocando: descarta (ninguém ouve)
            if !sfx_cooldown.try_start(sfx::play_time(frame.packets.len()), sfx::COOLDOWN_SLACK) {
                return;
            }

            // Quem tocou é sempre quem mandou (o id que veio do Client é ignorado)
            let name = sfx::clean_name(&frame.name);
            let relay = SfxFrame {
                user_id: id,
                name: name.clone(),
                packets: frame.packets,
            };
            let sfx_id = conn_inbox.put(relay.packets.clone());
            conn_registry.broadcast(&Message::Sfx(relay), Some(id));
            conn_events.emit(HostEvent::SfxPlayed {
                user_id: id,
                name,
                sfx_id,
            });
        }
        ConnectionEvent::Message(Message::StreamAnnounce { port }) => {
            if port == 0 {
                return;
            }
            // Na v2.0 quem transmite não assiste: sai das transmissões que estava vendo
            conn_shared.revoke_viewer_everywhere(id);
            conn_shared.announce_stream(
                id,
                StreamSpot {
                    ip: Some(addr.ip().to_string()),
                    port,
                },
            );
        }
        ConnectionEvent::Message(Message::StreamEnded) => conn_shared.end_stream(id),
        ConnectionEvent::Message(Message::WatchRequest { user_id }) => {
            conn_shared.watch_request(id, user_id)
        }
        ConnectionEvent::Message(_) => {}
    })
    .await;

    shared.registry.remove(id);

    // Se ele transmitia, a transmissão acaba; se assistia alguém, perde o acesso
    shared.end_stream(id);
    shared.revoke_viewer_everywhere(id);

    let was_kicked = shared.kicker.finish(id);

    // No encerramento do Host todo mundo sai junto; e quem foi expulso já foi anunciado
    if !*shutdown_check.borrow() && !was_kicked {
        let left = Message::UserLeft {
            user: UserInfo {
                id,
                name: name.clone(),
            },
        };
        shared.registry.broadcast(&left, None);
    }

    if was_kicked {
        shared.events.emit(HostEvent::UserKicked {
            id,
            name,
            addr: addr.to_string(),
        });
    } else {
        shared.events.emit(HostEvent::UserLeft {
            id,
            name,
            addr: addr.to_string(),
            reason: reason.to_string(),
        });
    }
}

/// Espera o Hello, confere a senha (se houver), registra o Client
/// e responde com Welcome ou Rejected.
async fn handshake(
    stream: &mut TcpStream,
    addr: SocketAddr,
    shared: &Shared,
    out_tx: UnboundedSender<Message>,
) -> io::Result<Option<ClientInfo>> {
    match read_message(stream).await? {
        Message::Hello {
            protocol_version,
            device_name,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                let motivo =
                    format!("versao do protocolo incompativel (host usa v{PROTOCOL_VERSION})");
                return refuse(
                    stream,
                    addr,
                    shared,
                    &device_name,
                    motivo,
                    REJECT_PROTOCOL_MISMATCH,
                )
                .await;
            }

            // Senha primeiro: quem não passa nunca chega a ser registrado
            if !check_password(stream, addr, shared, &device_name).await? {
                return Ok(None);
            }

            // Tenta registrar; None = sala cheia
            let Some(client) = shared.registry.try_add(&device_name, addr, out_tx) else {
                return refuse(
                    stream,
                    addr,
                    shared,
                    &device_name,
                    "sala cheia".to_string(),
                    REJECT_ROOM_FULL,
                )
                .await;
            };

            // O nome final pode ser diferente do pedido (limpeza ou repetido)
            if client.name != device_name {
                shared.events.info(format!(
                    "Nome '{device_name}' ajustado para '{}'",
                    client.name
                ));
            }

            let resposta = Message::Welcome {
                client_id: client.id,
                display_name: client.name.clone(),
                users: shared.registry.users(),
                host_name: shared.host_name.as_str().to_string(),
            };
            // Se não deu para responder, desfaz o registro
            if let Err(erro) = write_message(stream, &resposta).await {
                shared.registry.remove(client.id);
                return Err(erro);
            }

            Ok(Some(client))
        }

        outra => {
            shared
                .events
                .warn(format!("Mensagem inesperada no inicio: {outra:?}"));
            Ok(None)
        }
    }
}

/// Se a sala tem senha, pede ao Client (até MAX_PASSWORD_ATTEMPTS vezes).
/// Devolve true se pode entrar; false se foi recusado (a recusa já foi enviada).
async fn check_password(
    stream: &mut TcpStream,
    addr: SocketAddr,
    shared: &Shared,
    name: &str,
) -> io::Result<bool> {
    let Some(expected) = &shared.password else {
        return Ok(true); // sala sem senha
    };

    for attempt in 1..=MAX_PASSWORD_ATTEMPTS {
        write_message(stream, &Message::PasswordRequired).await?;

        let reply = match tokio::time::timeout(PASSWORD_TIMEOUT, read_message(stream)).await {
            Ok(reply) => reply?,
            Err(_) => {
                refuse(
                    stream,
                    addr,
                    shared,
                    name,
                    "tempo esgotado esperando a senha".to_string(),
                    REJECT_PASSWORD_TIMEOUT,
                )
                .await?;
                return Ok(false);
            }
        };

        match reply {
            Message::Password { password } if password == **expected => return Ok(true),

            Message::Password { .. } => {
                shared.events.emit(HostEvent::PasswordFailed {
                    name: name.to_string(),
                    addr: addr.to_string(),
                    attempt,
                    max_attempts: MAX_PASSWORD_ATTEMPTS,
                });
                tokio::time::sleep(WRONG_PASSWORD_DELAY).await;
            }

            outra => {
                shared
                    .events
                    .warn(format!("Mensagem inesperada ao pedir a senha: {outra:?}"));
                return Ok(false);
            }
        }
    }

    refuse(
        stream,
        addr,
        shared,
        name,
        "senha incorreta".to_string(),
        REJECT_WRONG_PASSWORD,
    )
    .await?;
    Ok(false)
}

/// Responde "recusado" ao Client e avisa por evento.
/// `reason` vai em português (Clients antigos mostram ele); `code` deixa o Client traduzir.
async fn refuse(
    stream: &mut TcpStream,
    addr: SocketAddr,
    shared: &Shared,
    name: &str,
    reason: String,
    code: &str,
) -> io::Result<Option<ClientInfo>> {
    shared.events.emit(HostEvent::UserRejected {
        name: name.to_string(),
        addr: addr.to_string(),
        reason: reason.clone(),
    });
    write_message(
        stream,
        &Message::Rejected {
            reason,
            code: code.to_string(),
        },
    )
    .await?;
    Ok(None)
}

/// Nome do Host na lista de usuários: o nome de usuário salvo ou, sem ele, o nome do computador.
fn host_display_name() -> String {
    settings::load()
        .map(|s| s.username)
        .unwrap_or_else(|| std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Host".to_string()))
}

/// Nome da sala: o pedido (limpo) ou, se vier vazio, o nome do computador.
fn room_name(requested: Option<String>) -> String {
    let cleaned: String = requested
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ROOM_NAME_LEN)
        .collect();

    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Host".to_string())
    } else {
        cleaned.to_string()
    }
}

/// Os IPs deste computador, para você saber o que digitar no Client.
fn local_addresses(events: &EventSender) -> Vec<LocalAddress> {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces
            .into_iter()
            .filter_map(|interface| match interface.ip() {
                IpAddr::V4(ip) if !interface.is_loopback() && !ip.is_link_local() => {
                    Some(LocalAddress {
                        ip: ip.to_string(),
                        interface: interface.name.clone(),
                    })
                }
                _ => None,
            })
            .collect(),
        Err(erro) => {
            events.warn(format!("Nao foi possivel listar os enderecos: {erro}"));
            Vec::new()
        }
    }
}
//! Motor do Host: abre a sala, aceita Clients e controla a transmissão da tela.
//!
//! Este módulo NÃO imprime nada e NÃO lê o teclado. Quem usa (o terminal ou a janela)
//! manda comandos pelos métodos de `HostEngine` e recebe o que acontece pelos eventos.

use crate::discovery::{self, RoomSettings};
use crate::events::{EventSender, HostEvent, LocalAddress, UserEntry};
use crate::kick::Kicker;
use crate::registry::{ClientInfo, Registry};
use crate::stream::{StreamHandle, StreamSettings};
use lanshare_core::protocol::{
    read_message, write_message, Message, UserInfo, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_net::{run_connection, ConnectionEvent};
use serde::Deserialize;
use std::io;
use std::net::{IpAddr, SocketAddr};
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

/// Dados compartilhados por todas as conexões.
#[derive(Clone)]
struct Shared {
    registry: Registry,
    kicker: Kicker,
    password: Option<Arc<String>>,
    events: EventSender,
}

/// A sala aberta. Tem o seu próprio runtime do tokio, então pode ser usada de qualquer thread.
pub struct HostEngine {
    runtime: Runtime,
    registry: Registry,
    kicker: Kicker,
    events: EventSender,
    event_rx: Mutex<Receiver<HostEvent>>,
    shutdown_tx: watch::Sender<bool>,
    /// A transmissão da tela (None = não está transmitindo).
    streaming: Mutex<Option<StreamHandle>>,
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

        let shared = Shared {
            registry: registry.clone(),
            kicker: kicker.clone(),
            password: password.clone(),
            events: events.clone(),
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

        // Aceita os Clients que chegarem
        runtime.spawn(accept_loop(listener, shutdown_rx, shared));

        Ok(Self {
            runtime,
            registry,
            kicker,
            events,
            event_rx: Mutex::new(event_rx),
            shutdown_tx,
            streaming: Mutex::new(None),
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

    /// Remove um Client: avisa ele, avisa os outros e encerra a conexão dele.
    /// Devolve o nome de quem foi removido, ou o motivo de não ter dado certo.
    pub fn kick(&self, id: u64) -> Result<String, String> {
        let Some(target) = self.registry.list().into_iter().find(|c| c.id == id) else {
            return Err(format!("Nao existe client com id #{id}."));
        };

        // O Kicker usa tokio::spawn, que precisa estar dentro do runtime
        let _guard = self.runtime.enter();
        if !self.kicker.kick(id, "Voce foi removido pelo Host") {
            return Err(format!("{} (#{id}) ja esta saindo.", target.name));
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

    /// Começa a transmitir a tela para os Clients.
    pub fn start_stream(&self, settings: StreamSettings) -> Result<(), String> {
        if !matches!(settings.height, 720 | 1080) || !matches!(settings.fps, 15 | 30) {
            return Err("Resolucao ou FPS invalidos (use 720 ou 1080, e 15 ou 30).".to_string());
        }

        let mut streaming = self.lock_streaming();
        if streaming.as_ref().is_some_and(|s| s.is_running()) {
            return Err("A tela ja esta sendo compartilhada.".to_string());
        }

        *streaming = Some(StreamHandle::start(
            self.registry.clone(),
            settings,
            self.events.clone(),
        ));
        Ok(())
    }

    /// Para a transmissão da tela. Devolve false se não havia transmissão.
    pub fn stop_stream(&self) -> bool {
        // Tira da caixa antes de esperar a thread terminar (para não segurar o lock)
        let handle = self.lock_streaming().take();

        match handle {
            Some(running) => {
                running.stop();
                true
            }
            None => false,
        }
    }

    /// A tela está sendo transmitida? (false também se a transmissão caiu por erro)
    pub fn is_streaming(&self) -> bool {
        self.lock_streaming()
            .as_ref()
            .is_some_and(|s| s.is_running())
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

    fn lock_streaming(&self) -> MutexGuard<'_, Option<StreamHandle>> {
        self.streaming.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Aceita Clients até a sala ser fechada.
async fn accept_loop(listener: TcpListener, shutdown: watch::Receiver<bool>, shared: Shared) {
    let mut stop = shutdown.clone();

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, client_addr)) => {
                    // Sem isso o TCP junta pacotes pequenos e atrasa o vídeo
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

    // Guardamos uma cópia para saber, no fim, se o Host inteiro está encerrando
    let shutdown_check = shutdown.clone();

    // O ping de cada Client é medido aqui e vira um evento
    let ping_events = shared.events.clone();
    let reason = run_connection(stream, conn_shutdown_rx, out_rx, move |event| {
        if let ConnectionEvent::Rtt(rtt) = event {
            ping_events.emit(HostEvent::Ping {
                id,
                rtt_ms: rtt.as_millis() as u64,
            });
        }
    })
    .await;

    shared.registry.remove(id);
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
                return refuse(stream, addr, shared, &device_name, motivo).await;
            }

            // Senha primeiro: quem não passa nunca chega a ser registrado
            if !check_password(stream, addr, shared, &device_name).await? {
                return Ok(None);
            }

            // Tenta registrar; None = sala cheia
            let Some(client) = shared.registry.try_add(&device_name, addr, out_tx) else {
                return refuse(stream, addr, shared, &device_name, "sala cheia".to_string()).await;
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

    refuse(stream, addr, shared, name, "senha incorreta".to_string()).await?;
    Ok(false)
}

/// Responde "recusado" ao Client e avisa por evento.
async fn refuse(
    stream: &mut TcpStream,
    addr: SocketAddr,
    shared: &Shared,
    name: &str,
    reason: String,
) -> io::Result<Option<ClientInfo>> {
    shared.events.emit(HostEvent::UserRejected {
        name: name.to_string(),
        addr: addr.to_string(),
        reason: reason.clone(),
    });
    write_message(stream, &Message::Rejected { reason }).await?;
    Ok(None)
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
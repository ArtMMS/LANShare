//! Motor do Client: procura salas, conecta num Host e recebe a tela.
//!
//! Não imprime nada e não lê o teclado. Quem usa (a interface) manda comandos pelos
//! métodos de `ClientEngine` e recebe o que acontece como eventos em JSON.

use crate::audio_player::{AudioOutputSetting, AudioPlayer, AudioVolume};
use crate::discovery;
use crate::normalize_address;
use crate::sync_clock::SyncClock;
use crate::video::{FrameSlot, VideoPipeline};
use lanshare_core::i18n::tr;
use lanshare_core::protocol::{
    read_message, write_message, Message, SfxFrame, UserInfo, VideoFrame, MAX_AVATAR_LEN,
    PROTOCOL_VERSION, REJECT_PASSWORD_TIMEOUT, REJECT_PROTOCOL_MISMATCH, REJECT_ROOM_FULL,
    REJECT_WRONG_PASSWORD,
};
use lanshare_core::settings;
use lanshare_core::sfx::{self, SfxCooldown, SfxInbox};
use lanshare_net::{run_connection, ConnectionEvent};
use serde_json::{json, Value};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self as std_mpsc, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

/// Canal por onde o motor avisa a interface (cada evento é um JSON com "type").
pub(crate) type EventTx = Sender<Value>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// De quanto em quanto tempo o motor avisa FPS e Mbps do vídeo recebido.
const STATS_INTERVAL: Duration = Duration::from_secs(2);

/// Tempo para o Bye chegar ao Host antes de fechar tudo.
const STOP_GRACE: Duration = Duration::from_millis(300);

/// Nome usado enquanto o usuário não tiver um salvo no settings.json.
const FALLBACK_USERNAME: &str = "Usuário";

fn send(events: &EventTx, event: Value) {
    let _ = events.send(event);
}

/// Controles da conexão em andamento.
struct Session {
    shutdown_tx: watch::Sender<bool>,
    password_tx: mpsc::UnboundedSender<String>,
    /// Caixa de saída: o que for posto aqui vai para o Host (depois de entrar na sala).
    outgoing_tx: mpsc::UnboundedSender<Message>,
}

/// O Client em execução. Tem o seu próprio runtime do tokio, então pode ser usado de qualquer thread.
pub struct ClientEngine {
    runtime: Runtime,
    events: EventTx,
    event_rx: Mutex<Receiver<Value>>,
    session: Arc<Mutex<Option<Session>>>,
    scanning: Arc<AtomicBool>,
    frame: FrameSlot,
    /// O meu avatar (PNG em base64), enviado ao entrar numa sala. None = sem avatar.
    avatar: Mutex<Option<String>>,
    /// Por onde sai o som das transmissões (escolhido nas Configurações).
    audio_output: AudioOutputSetting,
    /// Volume do som das transmissões (o botão sobre o vídeo).
    audio_volume: AudioVolume,
    /// Efeitos sonoros que chegaram da sala, esperando a interface buscar.
    sfx_inbox: SfxInbox,
    /// Anti-spam dos meus sons (o Host também confere: isto só evita mandar à toa).
    sfx_cooldown: Mutex<SfxCooldown>,
}

impl ClientEngine {
    pub fn start() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("lanshare-client")
            .build()?;

        let (events, event_rx) = std_mpsc::channel();

        Ok(Self {
            runtime,
            events,
            event_rx: Mutex::new(event_rx),
            session: Arc::new(Mutex::new(None)),
            scanning: Arc::new(AtomicBool::new(false)),
            frame: FrameSlot::default(),
            avatar: Mutex::new(None),
            audio_output: AudioOutputSetting::default(),
            audio_volume: AudioVolume::default(),
            sfx_inbox: SfxInbox::default(),
            sfx_cooldown: Mutex::new(SfxCooldown::default()),
        })
    }

    /// Próximo evento que ainda não foi lido (None = não há nenhum no momento).
    pub fn try_next_event(&self) -> Option<Value> {
        self.event_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv()
            .ok()
    }

    /// Define (ou apaga, com None) o meu avatar. Se já estiver numa sala, avisa o Host na hora.
    pub fn set_avatar(&self, png_base64: Option<String>) -> Result<(), String> {
        if png_base64.as_ref().is_some_and(|a| a.len() > MAX_AVATAR_LEN) {
            return Err(tr(
                "O avatar é grande demais para enviar.",
                "The avatar is too large to send.",
                "El avatar es demasiado grande para enviar.",
            )
            .to_string());
        }

        *self.avatar.lock().unwrap_or_else(|e| e.into_inner()) = png_base64.clone();

        if let Some(session) = self.lock_session().as_ref() {
            let _ = session.outgoing_tx.send(Message::Avatar {
                png_base64: png_base64.unwrap_or_default(),
            });
        }
        Ok(())
    }

    /// Escolhe a saída do som das transmissões (id do dispositivo; None = padrão do Windows).
    /// Se já houver som tocando, ele muda de saída na hora.
    pub fn set_audio_output(&self, device_id: Option<String>) {
        self.audio_output.set(device_id);
    }

    /// Volume do som das transmissões (0 a 100) e mudo. Muda na hora, mesmo tocando.
    pub fn set_volume(&self, percent: u32, muted: bool) {
        self.audio_volume.set(percent, muted);
    }

    /// Toca um efeito sonoro para a sala: vai ao Host, que repassa a todos (a interface já tocou ele aqui).
    /// `packets`: o som já comprimido com `sfx::encode` (comprima antes, fora de qualquer lock).
    /// Erro = não está numa sala, ou o som anterior ainda está tocando (anti-spam: um som por vez).
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
            user_id: 0, // o Host troca pelo meu id
            name: sfx::clean_name(name),
            packets: Arc::new(packets),
        };

        let sent = self
            .lock_session()
            .as_ref()
            .is_some_and(|session| session.outgoing_tx.send(Message::Sfx(frame)).is_ok());

        if sent {
            Ok(())
        } else {
            Err(tr(
                "Você não está numa sala.",
                "You are not in a room.",
                "No estás en ninguna sala.",
            )
            .to_string())
        }
    }

    /// Busca (e descomprime) um efeito sonoro avisado no evento "sfx_played".
    /// None = já foi buscado, foi descartado ou veio inválido.
    pub fn take_sfx(&self, sfx_id: u64) -> Option<Vec<f32>> {
        self.sfx_inbox.take(sfx_id)
    }

    /// Procura salas na rede (~2 s, em segundo plano). O resultado chega no evento "rooms".
    pub fn scan_rooms(&self) -> Result<(), String> {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return Err(tr(
                "Já está procurando salas.",
                "Already searching for rooms.",
                "Ya se están buscando salas.",
            )
            .to_string());
        }

        let scanning = self.scanning.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let event = match discovery::scan().await {
                Ok(rooms) => json!({
                    "type": "rooms",
                    "rooms": rooms.iter().map(room_json).collect::<Vec<_>>(),
                }),
                Err(erro) => json!({
                    "type": "scan_failed",
                    "error": format!(
                        "{}: {erro}",
                        tr(
                            "Não foi possível procurar salas",
                            "Could not search for rooms",
                            "No se pudieron buscar salas"
                        )
                    ),
                }),
            };
            scanning.store(false, Ordering::SeqCst);
            send(&events, event);
        });
        Ok(())
    }

    /// Conecta numa sala ("192.168.0.10" ou "192.168.0.10:47800"). O andamento vem por eventos.
    pub fn connect(&self, address: &str) -> Result<(), String> {
        let address = address.trim();
        if address.is_empty() {
            return Err(tr(
                "Informe o endereço da sala.",
                "Enter the room address.",
                "Introduce la dirección de la sala.",
            )
            .to_string());
        }
        let address = normalize_address(address);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (password_tx, password_rx) = mpsc::unbounded_channel();
        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel::<Message>();

        // O avatar sai logo depois que o Host aceitar (a caixa de saída só é lida após o handshake)
        let avatar = self.avatar.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(png_base64) = avatar {
            let _ = outgoing_tx.send(Message::Avatar { png_base64 });
        }

        {
            let mut session = self.lock_session();
            if session.is_some() {
                return Err(tr(
                    "Já existe uma conexão em andamento.",
                    "There is already a connection in progress.",
                    "Ya hay una conexión en curso.",
                )
                .to_string());
            }
            *session = Some(Session {
                shutdown_tx,
                password_tx,
                outgoing_tx,
            });
        }

        // Sem nome salvo, usa um provisório (sem salvar, para não atrapalhar a primeira execução)
        let username = settings::load()
            .map(|s| s.username)
            .unwrap_or_else(|| FALLBACK_USERNAME.to_string());

        let session_slot = self.session.clone();
        let events = self.events.clone();
        let frame = self.frame.clone();
        let audio_output = self.audio_output.clone();
        let audio_volume = self.audio_volume.clone();
        let sfx_inbox = self.sfx_inbox.clone();

        self.runtime.spawn(async move {
            let end = run_session(
                address,
                username,
                shutdown_rx,
                password_rx,
                outgoing_rx,
                events.clone(),
                frame.clone(),
                audio_output,
                audio_volume,
                sfx_inbox,
            )
            .await;

            // Libera a sessão ANTES de avisar: quem recebe o aviso já pode conectar de novo
            *session_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            frame.end_session();
            send(&events, end.into_event());
        });
        Ok(())
    }

    /// Responde ao pedido de senha ("password_required").
    pub fn submit_password(&self, password: String) -> Result<(), String> {
        let session = self.lock_session();
        match session.as_ref() {
            Some(s) => s
                .password_tx
                .send(password)
                .map_err(|_| {
                    tr(
                        "A conexão já foi encerrada.",
                        "The connection has already ended.",
                        "La conexión ya terminó.",
                    )
                    .to_string()
                }),
            None => Err(tr(
                "Não há conexão em andamento.",
                "There is no connection in progress.",
                "No hay ninguna conexión en curso.",
            )
            .to_string()),
        }
    }

    /// Desconecta (ou cancela a tentativa). O fim vem no evento "disconnected".
    /// Devolve false se não havia conexão.
    pub fn disconnect(&self) -> bool {
        let session = self.lock_session();
        match session.as_ref() {
            Some(s) => {
                let _ = s.shutdown_tx.send(true);
                true
            }
            None => false,
        }
    }

    /// Copia a imagem mais recente (BGRA) para `out`, só se for NOVA desde a última leitura.
    /// Devolve (largura, altura).
    pub fn frame_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        self.frame.read_into(out)
    }

    /// Desconecta e encerra o motor.
    pub fn stop(self) {
        if self.disconnect() {
            std::thread::sleep(STOP_GRACE);
        }
        self.runtime.shutdown_timeout(Duration::from_secs(2));
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Uma sala encontrada, no formato que a interface recebe.
fn room_json(room: &discovery::FoundRoom) -> Value {
    let info = &room.info;
    json!({
        "name": info.name,
        "address": room.address(),
        "users": info.users,
        "max_users": info.max_users,
        "has_password": info.has_password,
        "compatible": info.protocol_version == PROTOCOL_VERSION,
    })
}

/// Como uma tentativa de conexão terminou.
enum SessionEnd {
    Cancelled,
    Failed(String),
    Rejected(String),
    Ended { reason: String, local: bool },
}

impl SessionEnd {
    fn into_event(self) -> Value {
        match self {
            Self::Cancelled => json!({
                "type": "disconnected",
                "reason": tr("Conexão cancelada.", "Connection cancelled.", "Conexión cancelada."),
                "local": true,
            }),
            Self::Failed(error) => json!({ "type": "connect_failed", "error": error }),
            Self::Rejected(reason) => json!({ "type": "rejected", "reason": reason }),
            Self::Ended { reason, local } => json!({
                "type": "disconnected",
                "reason": reason,
                "local": local,
            }),
        }
    }
}

/// Resultado do handshake com o Host.
enum Handshake {
    Welcome {
        client_id: u64,
        display_name: String,
        users: Vec<UserInfo>,
        host_name: String,
    },
    Rejected { reason: String, code: String },
    Cancelled,
}

/// Motivo da recusa no idioma do app. Código desconhecido (Host antigo) = o texto que veio do Host.
fn rejection_text(code: &str, reason: String) -> String {
    let text = match code {
        REJECT_ROOM_FULL => tr("sala cheia", "the room is full", "la sala está llena"),
        REJECT_WRONG_PASSWORD => tr("senha incorreta", "wrong password", "contraseña incorrecta"),
        REJECT_PASSWORD_TIMEOUT => tr(
            "tempo esgotado esperando a senha",
            "timed out waiting for the password",
            "tiempo agotado esperando la contraseña",
        ),
        REJECT_PROTOCOL_MISMATCH => tr(
            "a sala usa uma versão incompatível do LANShare",
            "the room uses an incompatible version of LANShare",
            "la sala usa una versión incompatible de LANShare",
        ),
        _ => return reason,
    };
    text.to_string()
}

/// Conecta, entra na sala e mantém a conexão até ela acabar.
async fn run_session(
    address: String,
    username: String,
    mut shutdown: watch::Receiver<bool>,
    mut password_rx: mpsc::UnboundedReceiver<String>,
    outgoing_rx: mpsc::UnboundedReceiver<Message>,
    events: EventTx,
    frame: FrameSlot,
    audio_output: AudioOutputSetting,
    audio_volume: AudioVolume,
    sfx_inbox: SfxInbox,
) -> SessionEnd {
    send(&events, json!({ "type": "connecting", "address": address }));

    let connected = tokio::select! {
        result = timeout(CONNECT_TIMEOUT, TcpStream::connect(&address)) => result,
        _ = shutdown.changed() => return SessionEnd::Cancelled,
    };
    let mut stream = match connected {
        Ok(Ok(stream)) => stream,
        Ok(Err(erro)) => {
            return SessionEnd::Failed(format!(
                "{}: {erro}",
                tr(
                    "Não foi possível conectar",
                    "Could not connect",
                    "No se pudo conectar"
                )
            ))
        }
        Err(_) => {
            return SessionEnd::Failed(format!(
                "{} ({} s). {}",
                tr("Tempo esgotado", "Timed out", "Tiempo agotado"),
                CONNECT_TIMEOUT.as_secs(),
                tr(
                    "Confira o IP, o Firewall do Host e se a sala está aberta.",
                    "Check the IP, the Host's firewall and whether the room is open.",
                    "Revisa la IP, el firewall del Host y si la sala está abierta."
                )
            ))
        }
    };

    // Sem isso o TCP junta pacotes pequenos e atrasa o vídeo
    let _ = stream.set_nodelay(true);

    let (client_id, display_name, users, host_name) = match handshake(
        &mut stream,
        &username,
        &mut shutdown,
        &mut password_rx,
        &events,
    )
    .await
    {
        Ok(Handshake::Welcome {
            client_id,
            display_name,
            users,
            host_name,
        }) => (client_id, display_name, users, host_name),
        Ok(Handshake::Rejected { reason, code }) => {
            return SessionEnd::Rejected(rejection_text(&code, reason))
        }
        Ok(Handshake::Cancelled) => return SessionEnd::Cancelled,
        Err(erro) => {
            return SessionEnd::Failed(format!(
                "{}: {erro}",
                tr("Falha na conexão", "Connection failed", "Fallo en la conexión")
            ))
        }
    };

    send(
        &events,
        json!({
            "type": "connected",
            "client_id": client_id,
            "display_name": display_name,
            "host_name": host_name,
            "users": users
                .iter()
                .map(|u| json!({ "id": u.id, "name": u.name }))
                .collect::<Vec<_>>(),
        }),
    );

    // Número da "geração" do vídeo: muda quando a transmissão para, para descartar imagens velhas
    let mut generation = frame.begin_session();

    // O decodificador só nasce quando chegar o primeiro pacote de vídeo
    let mut video: Option<VideoPipeline> = None;

    // O som também só nasce quando chegar o primeiro pacote de áudio
    let mut audio: Option<AudioPlayer> = None;

    // Relógio da sincronia: o som anota, o vídeo segue. Um novo a cada transmissão.
    let mut clock = SyncClock::default();
    let mut stats = VideoStats::new();
    let cb_events = events.clone();
    let cb_frame = frame.clone();

    let reason = run_connection(stream, shutdown.clone(), outgoing_rx, move |event| match event {
        ConnectionEvent::Rtt(rtt) => send(
            &cb_events,
            json!({ "type": "ping", "rtt_ms": rtt.as_millis() as u64 }),
        ),
        ConnectionEvent::Message(message) => match message {
            Message::Video(video_frame) => {
                if let Some(report) = stats.record(&video_frame) {
                    send(&cb_events, report);
                }
                video
                    .get_or_insert_with(|| {
                        VideoPipeline::start(
                            cb_frame.clone(),
                            generation,
                            cb_events.clone(),
                            clock.clone(),
                        )
                    })
                    .push(video_frame);
            }
            Message::Audio(audio_frame) => {
                audio
                    .get_or_insert_with(|| {
                        AudioPlayer::start(
                            audio_output.clone(),
                            audio_volume.clone(),
                            cb_events.clone(),
                            clock.clone(),
                        )
                    })
                    .push(audio_frame);
            }
            Message::StreamStopped => {
                // O Host parou de transmitir: o decodificador e o som antigos terminam sozinhos
                video = None;
                audio = None;
                clock = SyncClock::default();
                // Imagens que ainda estavam sendo decodificadas são descartadas
                generation = cb_frame.begin_session();
                stats = VideoStats::new();
                send(&cb_events, json!({ "type": "stream_stopped" }));
            }
            Message::UserJoined { user } => send(
                &cb_events,
                json!({ "type": "user_joined", "id": user.id, "name": user.name }),
            ),
            Message::UserLeft { user } => send(
                &cb_events,
                json!({ "type": "user_left", "id": user.id, "name": user.name }),
            ),
            Message::UserKicked { user } => send(
                &cb_events,
                json!({ "type": "user_kicked", "id": user.id, "name": user.name }),
            ),
            Message::UserAvatar { id, png_base64 } => send(
                &cb_events,
                json!({ "type": "user_avatar", "id": id, "png_base64": png_base64 }),
            ),
            // Alguém tocou um efeito sonoro: guarda o som e avisa quem foi (a interface busca e toca)
            Message::Sfx(SfxFrame {
                user_id,
                name,
                packets,
            }) => {
                let sfx_id = sfx_inbox.put(packets);
                send(
                    &cb_events,
                    json!({
                        "type": "sfx_played",
                        "user_id": user_id,
                        "name": sfx::clean_name(&name),
                        "sfx_id": sfx_id,
                    }),
                );
            }
            // O motivo que vem da rede está no idioma do Host: usa o texto no idioma deste app
            Message::Kicked { .. } => send(
                &cb_events,
                json!({
                    "type": "kicked",
                    "reason": tr(
                        "Você foi removido pelo Host.",
                        "You were removed by the Host.",
                        "El Host te ha expulsado de la sala.",
                    ),
                }),
            ),
            _ => {}
        },
    })
    .await;

    SessionEnd::Ended {
        reason: reason.to_string(),
        local: *shutdown.borrow(),
    }
}

/// Manda o Hello e responde ao pedido de senha até o Host aceitar ou recusar.
async fn handshake(
    stream: &mut TcpStream,
    username: &str,
    shutdown: &mut watch::Receiver<bool>,
    password_rx: &mut mpsc::UnboundedReceiver<String>,
    events: &EventTx,
) -> io::Result<Handshake> {
    let hello = Message::Hello {
        protocol_version: PROTOCOL_VERSION,
        device_name: username.to_string(),
    };
    write_message(stream, &hello).await?;

    let mut asked_before = false;

    loop {
        let message = tokio::select! {
            message = read_message(stream) => message?,
            _ = shutdown.changed() => return Ok(Handshake::Cancelled),
        };

        match message {
            Message::Welcome {
                client_id,
                display_name,
                users,
                host_name,
            } => {
                return Ok(Handshake::Welcome {
                    client_id,
                    display_name,
                    users,
                    host_name,
                })
            }

            Message::PasswordRequired => {
                // A interface mostra o campo de senha só agora
                send(
                    events,
                    json!({ "type": "password_required", "retry": asked_before }),
                );
                asked_before = true;

                let password = tokio::select! {
                    password = password_rx.recv() => match password {
                        Some(password) => password,
                        None => return Ok(Handshake::Cancelled),
                    },
                    _ = shutdown.changed() => return Ok(Handshake::Cancelled),
                };
                write_message(stream, &Message::Password { password }).await?;
            }

            Message::Rejected { reason, code } => {
                return Ok(Handshake::Rejected { reason, code })
            }

            outra => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{}: {outra:?}",
                        tr(
                            "resposta inesperada do Host",
                            "unexpected reply from the Host",
                            "respuesta inesperada del Host"
                        )
                    ),
                ))
            }
        }
    }
}

/// Conta o vídeo que chega e gera o evento "video_stats" de tempos em tempos.
struct VideoStats {
    started: bool,
    since: Instant,
    frames: u32,
    bytes: u64,
}

impl VideoStats {
    fn new() -> Self {
        Self {
            started: false,
            since: Instant::now(),
            frames: 0,
            bytes: 0,
        }
    }

    fn record(&mut self, frame: &VideoFrame) -> Option<Value> {
        if !self.started {
            self.started = true;
            self.since = Instant::now();
        }

        self.frames += 1;
        self.bytes += frame.data.len() as u64;

        let elapsed = self.since.elapsed();
        if elapsed < STATS_INTERVAL {
            return None;
        }

        let secs = elapsed.as_secs_f64();
        let report = json!({
            "type": "video_stats",
            "fps": self.frames as f64 / secs,
            "mbps": self.bytes as f64 * 8.0 / secs / 1_000_000.0,
        });

        self.since = Instant::now();
        self.frames = 0;
        self.bytes = 0;
        Some(report)
    }
}





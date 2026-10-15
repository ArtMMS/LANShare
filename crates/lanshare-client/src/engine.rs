//! Motor do Client: procura salas, conecta num Host e recebe a tela.
//!
//! Não imprime nada e não lê o teclado. Quem usa (a interface) manda comandos pelos
//! métodos de `ClientEngine` e recebe o que acontece como eventos em JSON.

use crate::discovery;
use crate::normalize_address;
use crate::video::{FrameSlot, VideoPipeline};
use lanshare_core::protocol::{
    read_message, write_message, Message, UserInfo, VideoFrame, PROTOCOL_VERSION,
};
use lanshare_core::settings;
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
}

/// O Client em execução. Tem o seu próprio runtime do tokio, então pode ser usado de qualquer thread.
pub struct ClientEngine {
    runtime: Runtime,
    events: EventTx,
    event_rx: Mutex<Receiver<Value>>,
    session: Arc<Mutex<Option<Session>>>,
    scanning: Arc<AtomicBool>,
    frame: FrameSlot,
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

    /// Procura salas na rede (~2 s, em segundo plano). O resultado chega no evento "rooms".
    pub fn scan_rooms(&self) -> Result<(), String> {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return Err("Já está procurando salas.".to_string());
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
                    "error": format!("Não foi possível procurar salas: {erro}"),
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
            return Err("Informe o endereço da sala.".to_string());
        }
        let address = normalize_address(address);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (password_tx, password_rx) = mpsc::unbounded_channel();

        {
            let mut session = self.lock_session();
            if session.is_some() {
                return Err("Já existe uma conexão em andamento.".to_string());
            }
            *session = Some(Session {
                shutdown_tx,
                password_tx,
            });
        }

        // Sem nome salvo, usa um provisório (sem salvar, para não atrapalhar a primeira execução)
        let username = settings::load()
            .map(|s| s.username)
            .unwrap_or_else(|| FALLBACK_USERNAME.to_string());

        let session_slot = self.session.clone();
        let events = self.events.clone();
        let frame = self.frame.clone();

        self.runtime.spawn(async move {
            let end = run_session(
                address,
                username,
                shutdown_rx,
                password_rx,
                events.clone(),
                frame.clone(),
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
                .map_err(|_| "A conexão já foi encerrada.".to_string()),
            None => Err("Não há conexão em andamento.".to_string()),
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
                "reason": "Conexão cancelada.",
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
    },
    Rejected(String),
    Cancelled,
}

/// Conecta, entra na sala e mantém a conexão até ela acabar.
async fn run_session(
    address: String,
    username: String,
    mut shutdown: watch::Receiver<bool>,
    mut password_rx: mpsc::UnboundedReceiver<String>,
    events: EventTx,
    frame: FrameSlot,
) -> SessionEnd {
    send(&events, json!({ "type": "connecting", "address": address }));

    let connected = tokio::select! {
        result = timeout(CONNECT_TIMEOUT, TcpStream::connect(&address)) => result,
        _ = shutdown.changed() => return SessionEnd::Cancelled,
    };
    let mut stream = match connected {
        Ok(Ok(stream)) => stream,
        Ok(Err(erro)) => return SessionEnd::Failed(format!("Não foi possível conectar: {erro}")),
        Err(_) => {
            return SessionEnd::Failed(format!(
                "Tempo esgotado ({} s). Confira o IP, o Firewall do Host e se a sala está aberta.",
                CONNECT_TIMEOUT.as_secs()
            ))
        }
    };

    // Sem isso o TCP junta pacotes pequenos e atrasa o vídeo
    let _ = stream.set_nodelay(true);

    let (client_id, display_name, users) = match handshake(
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
        }) => (client_id, display_name, users),
        Ok(Handshake::Rejected(reason)) => return SessionEnd::Rejected(reason),
        Ok(Handshake::Cancelled) => return SessionEnd::Cancelled,
        Err(erro) => return SessionEnd::Failed(format!("Falha na conexão: {erro}")),
    };

    send(
        &events,
        json!({
            "type": "connected",
            "client_id": client_id,
            "display_name": display_name,
            "users": users
                .iter()
                .map(|u| json!({ "id": u.id, "name": u.name }))
                .collect::<Vec<_>>(),
        }),
    );

    let generation = frame.begin_session();

    // O Client só envia Ping/Pong/Bye, mas a caixa de saída precisa existir
    let (_outgoing_tx, outgoing_rx) = mpsc::unbounded_channel::<Message>();

    // O decodificador só nasce quando chegar o primeiro pacote de vídeo
    let mut video: Option<VideoPipeline> = None;
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
                        VideoPipeline::start(cb_frame.clone(), generation, cb_events.clone())
                    })
                    .push(video_frame);
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
            Message::Kicked { reason } => {
                send(&cb_events, json!({ "type": "kicked", "reason": reason }))
            }
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
            } => {
                return Ok(Handshake::Welcome {
                    client_id,
                    display_name,
                    users,
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

            Message::Rejected { reason } => return Ok(Handshake::Rejected(reason)),

            outra => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("resposta inesperada do Host: {outra:?}"),
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
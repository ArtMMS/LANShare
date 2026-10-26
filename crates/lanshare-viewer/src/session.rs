//! Assistir UMA transmissão: conecta direto em quem transmite, entrega o ingresso
//! e passa o vídeo e o som para o VideoPipeline e o AudioPlayer.
//!
//! Eventos (JSON, todos com "user_id" e "watch_id"): watch_started, watch_failed, watch_ended.
//! O vídeo e o som também avisam: video_started, video_stats, sync_stats, video_error, audio_error.

use crate::audio_player::{AudioOutputSetting, AudioPlayer, AudioVolume};
use crate::sync_clock::SyncClock;
use crate::video::{FrameSlot, VideoPipeline};
use crate::EventTx;
use lanshare_core::i18n::tr;
use lanshare_core::protocol::{
    read_message, write_message, Message, VideoFrame, PROTOCOL_VERSION, REJECT_INVALID_TICKET,
};
use lanshare_net::{run_connection, ConnectionEvent};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Tempo para quem transmite responder ao WatchHello.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// O ingresso pode chegar em quem transmite um instante depois de quem assiste:
/// recusado por "ingresso inválido", tenta de novo algumas vezes.
const TICKET_ATTEMPTS: u32 = 4;
const TICKET_RETRY_DELAY: Duration = Duration::from_millis(300);

/// De quanto em quanto tempo avisa FPS e Mbps do vídeo recebido.
const STATS_INTERVAL: Duration = Duration::from_secs(2);

/// Quem assistir e como entrar.
pub(crate) struct WatchTarget {
    /// Número desta tentativa de assistir (a interface ignora avisos de tentativas antigas).
    pub watch_id: u64,
    /// Quem transmite (0 = o Host).
    pub user_id: u64,
    pub addr: SocketAddr,
    pub ticket: String,
}

/// Uma conexão de transmissão em andamento. Ao ser descartada, a conexão é encerrada.
pub(crate) struct WatchSession {
    pub user_id: u64,
    /// A imagem mais recente desta transmissão.
    pub frame: FrameSlot,
    shutdown: watch::Sender<bool>,
    /// true = quem transmite parou (o fim não foi escolha de quem assiste).
    stream_ended: Arc<AtomicBool>,
    /// true = a conexão já acabou.
    finished: Arc<AtomicBool>,
}

impl WatchSession {
    pub fn start(
        runtime: &Handle,
        target: WatchTarget,
        output: AudioOutputSetting,
        volume: AudioVolume,
        events: EventTx,
    ) -> Self {
        let (shutdown, shutdown_rx) = watch::channel(false);
        let frame = FrameSlot::default();
        let stream_ended = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let user_id = target.user_id;

        let task_frame = frame.clone();
        let task_ended = stream_ended.clone();
        let task_finished = finished.clone();
        runtime.spawn(async move {
            run(target, shutdown_rx, task_frame, task_ended, output, volume, events).await;
            task_finished.store(true, Ordering::SeqCst);
        });

        Self {
            user_id,
            frame,
            shutdown,
            stream_ended,
            finished,
        }
    }

    /// Para de assistir. `stream_ended` = a transmissão acabou (a sala avisou).
    pub fn end(&self, stream_ended: bool) {
        if stream_ended {
            self.stream_ended.store(true, Ordering::SeqCst);
        }
        let _ = self.shutdown.send(true);
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

fn send(events: &EventTx, event: Value) {
    let _ = events.send(event);
}

async fn run(
    target: WatchTarget,
    mut shutdown: watch::Receiver<bool>,
    frame: FrameSlot,
    stream_ended: Arc<AtomicBool>,
    output: AudioOutputSetting,
    volume: AudioVolume,
    events: EventTx,
) {
    let WatchTarget {
        watch_id,
        user_id,
        addr,
        ticket,
    } = target;

    let stream = tokio::select! {
        result = open(addr, &ticket) => match result {
            Ok(stream) => stream,
            Err(error) => {
                send(&events, json!({
                    "type": "watch_failed",
                    "user_id": user_id,
                    "watch_id": watch_id,
                    "error": error,
                }));
                return;
            }
        },
        // Desistiu antes de conectar (trocou de transmissão ou saiu da sala)
        _ = shutdown.changed() => return,
    };

    send(
        &events,
        json!({ "type": "watch_started", "user_id": user_id, "watch_id": watch_id }),
    );

    let generation = frame.begin_session();
    // Relógio da sincronia: o som anota, o vídeo segue
    let clock = SyncClock::default();
    // O decodificador e o som só nascem quando chega o primeiro pacote de cada um
    let mut video: Option<VideoPipeline> = None;
    let mut audio: Option<AudioPlayer> = None;
    let mut stats = VideoStats::new();
    let mut streamer_stopped = false;

    // Quem assiste não manda nada além de Ping/Pong/Bye
    let (_outgoing_tx, outgoing_rx) = mpsc::unbounded_channel::<Message>();

    let reason = run_connection(stream, shutdown.clone(), outgoing_rx, |event| {
        let ConnectionEvent::Message(message) = event else {
            return;
        };
        match message {
            Message::Video(video_frame) => {
                if let Some(report) = stats.record(&video_frame) {
                    send(&events, report);
                }
                video
                    .get_or_insert_with(|| {
                        VideoPipeline::start(frame.clone(), generation, events.clone(), clock.clone())
                    })
                    .push(video_frame);
            }
            Message::Audio(audio_frame) => {
                audio
                    .get_or_insert_with(|| {
                        AudioPlayer::start(output.clone(), volume.clone(), events.clone(), clock.clone())
                    })
                    .push(audio_frame);
            }
            // Quem transmite parou: a conexão vai ser encerrada em seguida
            Message::StreamStopped => streamer_stopped = true,
            _ => {}
        }
    })
    .await;

    // O decodificador e o som terminam sozinhos ao serem descartados
    drop(video);
    drop(audio);
    frame.end_session();

    let stopped = streamer_stopped || stream_ended.load(Ordering::SeqCst);
    let local = *shutdown.borrow() && !stopped;
    send(
        &events,
        json!({
            "type": "watch_ended",
            "user_id": user_id,
            "watch_id": watch_id,
            "stream_stopped": stopped,
            "local": local,
            "reason": reason.to_string(),
        }),
    );
}

/// Conecta em quem transmite e entrega o ingresso (tentando de novo se ele ainda não chegou lá).
async fn open(addr: SocketAddr, ticket: &str) -> Result<TcpStream, String> {
    let lost = |erro: std::io::Error| {
        format!(
            "{}: {erro}",
            tr(
                "A conexão com quem transmite caiu",
                "The connection to the streamer dropped",
                "Se cayó la conexión con quien transmite"
            )
        )
    };

    for attempt in 1..=TICKET_ATTEMPTS {
        let mut stream = match timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(erro)) => {
                return Err(format!(
                    "{} ({addr}): {erro}",
                    tr(
                        "Não foi possível conectar em quem transmite",
                        "Could not connect to the streamer",
                        "No se pudo conectar con quien transmite"
                    )
                ))
            }
            Err(_) => {
                return Err(format!(
                    "{} ({addr}). {}",
                    tr(
                        "Quem transmite não respondeu",
                        "The streamer did not respond",
                        "Quien transmite no respondió"
                    ),
                    tr(
                        "O Firewall do PC dele pode estar bloqueando o LANShare.",
                        "Their PC's firewall may be blocking LANShare.",
                        "El firewall de su PC puede estar bloqueando LANShare."
                    )
                ))
            }
        };

        // Sem isso o TCP junta pacotes pequenos e atrasa o vídeo
        let _ = stream.set_nodelay(true);

        let hello = Message::WatchHello {
            protocol_version: PROTOCOL_VERSION,
            ticket: ticket.to_string(),
        };
        write_message(&mut stream, &hello).await.map_err(lost)?;

        match timeout(HELLO_TIMEOUT, read_message(&mut stream)).await {
            Ok(Ok(Message::WatchAccepted)) => return Ok(stream),
            Ok(Ok(Message::Rejected { code, .. }))
                if code == REJECT_INVALID_TICKET && attempt < TICKET_ATTEMPTS =>
            {
                tokio::time::sleep(TICKET_RETRY_DELAY).await;
            }
            Ok(Ok(Message::Rejected { reason, .. })) => {
                return Err(format!(
                    "{}: {reason}",
                    tr(
                        "Quem transmite recusou a conexão",
                        "The streamer refused the connection",
                        "Quien transmite rechazó la conexión"
                    )
                ))
            }
            Ok(Ok(outra)) => {
                return Err(format!(
                    "{}: {outra:?}",
                    tr(
                        "Resposta inesperada de quem transmite",
                        "Unexpected reply from the streamer",
                        "Respuesta inesperada de quien transmite"
                    )
                ))
            }
            Ok(Err(erro)) => return Err(lost(erro)),
            Err(_) => {
                return Err(tr(
                    "Quem transmite demorou demais para responder.",
                    "The streamer took too long to respond.",
                    "Quien transmite tardó demasiado en responder.",
                )
                .to_string())
            }
        }
    }

    Err(tr(
        "O ingresso para assistir não foi aceito. Tente de novo.",
        "The watch ticket was not accepted. Try again.",
        "No se aceptó la entrada para ver. Inténtalo de nuevo.",
    )
    .to_string())
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
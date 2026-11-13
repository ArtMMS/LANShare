//! Transmissão da webcam (v2.0): câmera -> dois encoders de GPU -> duas portas.
//!
//! - versão pequena (`StreamQuality::Tile`): até 720p a até 30 FPS, para os quadrados da sala.
//!   Todo mundo da sala assiste esta;
//! - versão cheia (`StreamQuality::Full`): no tamanho e FPS escolhidos (até 4K / 60 FPS), só para
//!   quem abre a câmera grande.
//!
//! Cada versão tem o seu StreamServer (porta própria) e o seu encoder. Um encoder só nasce quando
//! a porta dele ganha o primeiro espectador: sem ninguém abrindo a câmera grande, a versão cheia
//! não gasta a GPU. Cada versão tem o seu bitrate adaptativo. A câmera não leva som.
//!
//! Usado pelo Host e pelo Client (quem liga a câmera); a prévia vai para um `PreviewSlot`.

use crate::camera::{CameraCapture, CameraRequest};
use crate::capture::CapturedFrame;
use crate::events::{StreamEvent, StreamEvents};
use crate::hw_encoder::{bitrate_for, HwEncoder};
use crate::preview::PreviewSlot;
use crate::server::StreamServer;
use crate::sink::StreamSink;
use crate::stream::{is_keyframe, mbps, AdaptiveBitrate};
use lanshare_core::i18n::tr;
use lanshare_core::protocol::StreamQuality;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

type BoxError = Box<dyn Error + Send + Sync>;

/// Altura e FPS máximos da versão pequena (quadrados da sala).
pub const CAMERA_TILE_HEIGHT: u32 = 720;
pub const CAMERA_TILE_FPS: u32 = 30;

/// Altura da prévia da minha câmera na interface (o meu quadrado da sala).
const CAMERA_PREVIEW_HEIGHT: u32 = 720;

/// De quanto em quanto tempo sai o resumo (e o bitrate adaptativo decide).
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Quantos horários de captura podem esperar o encoder devolver o pacote correspondente.
const MAX_PENDING_TIMESTAMPS: usize = 8;

/// O que pedir à câmera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraSettings {
    /// Qual câmera (o id de `list_cameras`). None = a primeira.
    pub device_id: Option<String>,
    /// Altura pedida (até 2160). A câmera usa a mais perto que tiver.
    pub height: u32,
    /// FPS pedido (até 60).
    pub fps: u32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            device_id: None,
            height: 1080,
            fps: 30,
        }
    }
}

/// A minha câmera no ar: as duas portas e a thread que captura e comprime.
pub struct CameraBroadcast {
    full: StreamServer,
    tile: StreamServer,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CameraBroadcast {
    /// Liga a câmera e abre as duas portas. Volta só depois de a câmera abrir (ou dar erro:
    /// sem câmera, câmera em uso por outro app...).
    pub fn start(
        runtime: &Handle,
        events: StreamEvents,
        preview: PreviewSlot,
        settings: CameraSettings,
    ) -> Result<Self, String> {
        let request = CameraRequest {
            height: settings.height.clamp(144, 2160),
            fps: settings.fps.clamp(5, 60),
        };
        let capture = CameraCapture::start(settings.device_id.clone(), request).map_err(|erro| {
            format!(
                "{}: {erro}",
                tr(
                    "Não foi possível ligar a câmera",
                    "Could not turn on the camera",
                    "No se pudo encender la cámara"
                )
            )
        })?;

        let open_port = |what: &str| {
            StreamServer::start(runtime, events.clone()).map_err(|erro| {
                format!(
                    "{} ({what}): {erro}",
                    tr(
                        "Não foi possível abrir a porta da câmera",
                        "Could not open the camera port",
                        "No se pudo abrir el puerto de la cámara"
                    )
                )
            })
        };
        let full = open_port("full")?;
        let tile = match open_port("tile") {
            Ok(tile) => tile,
            Err(erro) => {
                full.stop();
                return Err(erro);
            }
        };

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (thread_full, thread_tile) = (full.clone(), tile.clone());
        let max_fps = request.fps;

        // Os encoders de GPU precisam nascer e morrer na mesma thread: tudo fica dentro de run()
        let thread = std::thread::Builder::new()
            .name("lanshare-camera-stream".into())
            .spawn(move || {
                let result = run(
                    capture,
                    thread_full,
                    thread_tile,
                    max_fps,
                    thread_stop,
                    &events,
                    &preview,
                );
                preview.clear(); // a prévia some junto com a câmera
                events.emit(StreamEvent::Stopped {
                    error: result.err().map(|erro| erro.to_string()),
                });
            })
            .map_err(|erro| erro.to_string());

        match thread {
            Ok(thread) => Ok(Self {
                full,
                tile,
                stop,
                thread: Some(thread),
            }),
            Err(erro) => {
                full.stop();
                tile.stop();
                Err(erro)
            }
        }
    }

    /// Porta da versão cheia (vai no StreamAnnounce como `port`).
    pub fn full_port(&self) -> u16 {
        self.full.port()
    }

    /// Porta da versão pequena (vai no StreamAnnounce como `tile_port`).
    pub fn tile_port(&self) -> u16 {
        self.tile.port()
    }

    /// Autoriza um espectador na versão `quality`. false = ingresso inválido ou câmera parada.
    pub fn allow_viewer(&self, quality: StreamQuality, viewer_id: u64, ticket: String) -> bool {
        match quality {
            StreamQuality::Full => self.full.allow_viewer(viewer_id, ticket),
            StreamQuality::Tile => self.tile.allow_viewer(viewer_id, ticket),
        }
    }

    /// Derruba um espectador das duas versões.
    pub fn revoke_viewer(&self, viewer_id: u64) {
        self.full.revoke_viewer(viewer_id);
        self.tile.revoke_viewer(viewer_id);
    }

    /// false se a câmera parou sozinha (desconectada ou com erro).
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Fecha as portas (avisa quem assiste), desliga a câmera e espera a thread terminar.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.full.stop();
        self.tile.stop();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // Parada pela própria thread (num evento dela): não dá para esperar a si mesma
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for CameraBroadcast {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Bitrate da câmera: o mesmo das telas em 720p/1080p; acima disso cresce com os pixels e o FPS.
fn camera_bitrate(height: u32, fps: u32) -> u32 {
    if height <= 1080 && fps <= 30 {
        return bitrate_for(if height <= 720 { 720 } else { 1080 }, fps);
    }
    let pixels = (height as f64 / 1080.0).powi(2);
    let motion = (fps as f64 / 30.0).powf(0.7);
    ((6_000_000.0 * pixels.max(0.45) * motion) as u32).clamp(1_500_000, 40_000_000)
}

/// Uma versão da câmera: a porta, o encoder e a agenda de FPS dela.
struct Lane {
    sink: StreamServer,
    /// Altura e FPS desta versão.
    height: u32,
    fps: u32,
    encoder: Option<HwEncoder>,
    adaptive: AdaptiveBitrate,
    frame_interval: Duration,
    next_frame_at: Instant,
    pending_timestamps: VecDeque<u64>,
    packets: u32,
    bytes: u64,
}

impl Lane {
    fn new(sink: StreamServer, height: u32, fps: u32) -> Self {
        let fps = fps.max(1);
        Self {
            sink,
            height,
            fps,
            encoder: None,
            adaptive: AdaptiveBitrate::new(camera_bitrate(height, fps)),
            frame_interval: Duration::from_secs_f64(1.0 / fps as f64),
            next_frame_at: Instant::now(),
            pending_timestamps: VecDeque::new(),
            packets: 0,
            bytes: 0,
        }
    }

    /// Comprime e envia o frame, se alguém assiste esta versão e já está na hora (limite de FPS).
    fn push(&mut self, frame: &CapturedFrame, started: Instant) -> Result<(), BoxError> {
        if !self.sink.has_viewers() {
            return Ok(());
        }

        // Chegou antes da hora: pula (folga de 1/4 de intervalo para o jitter)
        let now = Instant::now();
        if now + self.frame_interval / 4 < self.next_frame_at {
            return Ok(());
        }
        self.next_frame_at = if now > self.next_frame_at + self.frame_interval {
            now + self.frame_interval
        } else {
            self.next_frame_at + self.frame_interval
        };

        if self.encoder.is_none() {
            self.encoder = Some(HwEncoder::new(
                frame.width,
                frame.height,
                self.height,
                self.fps,
                self.adaptive.current(),
            )?);
        }
        let Some(encoder) = self.encoder.as_mut() else {
            return Ok(());
        };

        // Alguém entrou ou ficou para trás: o próximo frame sai como keyframe
        if self.sink.take_keyframe_request() {
            encoder.force_keyframe();
        }

        let captured_us = frame.captured_at.saturating_duration_since(started).as_micros() as u64;
        self.pending_timestamps.push_back(captured_us);
        while self.pending_timestamps.len() > MAX_PENDING_TIMESTAMPS {
            self.pending_timestamps.pop_front();
        }

        for packet in encoder.encode(frame)? {
            let keyframe = is_keyframe(&packet);
            let timestamp_us = self.pending_timestamps.pop_front().unwrap_or(captured_us);
            self.packets += 1;
            self.bytes += packet.len() as u64;
            self.sink.send_video(keyframe, timestamp_us, Arc::new(packet));
        }
        Ok(())
    }

    /// A cada segundo: bitrate adaptativo (só quando o encoder existe) e zera os contadores.
    fn report(&mut self) -> (u32, u64) {
        let congestion = self.sink.take_congestion();
        if let Some(encoder) = self.encoder.as_mut() {
            if let Some(bitrate) = self.adaptive.update(congestion) {
                encoder.set_bitrate(bitrate);
            }
        }
        let counted = (self.packets, self.bytes);
        self.packets = 0;
        self.bytes = 0;
        counted
    }
}

fn run(
    capture: CameraCapture,
    full: StreamServer,
    tile: StreamServer,
    max_fps: u32,
    stop: Arc<AtomicBool>,
    events: &StreamEvents,
    preview: &PreviewSlot,
) -> Result<(), BoxError> {
    let mode = capture.mode.clone();
    let camera_fps = mode.fps_rounded().clamp(1, max_fps.max(1));

    let mut lanes = [
        Lane::new(
            tile,
            mode.height.min(CAMERA_TILE_HEIGHT),
            camera_fps.min(CAMERA_TILE_FPS),
        ),
        Lane::new(full, mode.height, camera_fps),
    ];

    events.emit(StreamEvent::Started {
        source_width: mode.width,
        source_height: mode.height,
        width: mode.width,
        height: mode.height,
        fps: camera_fps,
    });
    events.debug(format!(
        "camera: {} {}x{} @ {:.1} FPS ({})",
        capture.name,
        mode.width,
        mode.height,
        mode.fps(),
        mode.format
    ));

    // Relógio da câmera: os timestamps dos pacotes contam a partir daqui
    let started = Instant::now();
    let preview_interval = Duration::from_secs_f64(1.0 / camera_fps.min(CAMERA_TILE_FPS) as f64);
    let mut last_preview: Option<Instant> = None;
    let mut report_start = Instant::now();

    let result = loop {
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }

        let frame = match capture.frames.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => {
                if capture.is_running() {
                    continue;
                }
                break Err(camera_lost());
            }
            Err(RecvTimeoutError::Disconnected) => break Err(camera_lost()),
        };

        let mut failed = None;
        for lane in lanes.iter_mut() {
            if let Err(erro) = lane.push(&frame, started) {
                failed = Some(erro);
                break;
            }
        }
        if let Some(erro) = failed {
            break Err(erro);
        }

        // Prévia (o meu quadrado da sala), no máximo a 30 FPS
        let want_preview =
            last_preview.map_or(true, |t| t.elapsed() + preview_interval / 4 >= preview_interval);
        if want_preview {
            preview.store(frame, CAMERA_PREVIEW_HEIGHT);
            last_preview = Some(Instant::now());
        }

        if report_start.elapsed() >= REPORT_INTERVAL {
            let secs = report_start.elapsed().as_secs_f64();
            let [tile_lane, full_lane] = &mut lanes;
            let (tile_packets, tile_bytes) = tile_lane.report();
            let (full_packets, full_bytes) = full_lane.report();

            // Quem assiste grande também conta; a pequena todo mundo da sala recebe
            let viewers = tile_lane.sink.viewer_count().max(full_lane.sink.viewer_count());
            let active = if full_lane.encoder.is_some() && full_lane.sink.has_viewers() {
                &*full_lane
            } else {
                &*tile_lane
            };

            if tile_packets + full_packets > 0 || viewers > 0 {
                events.emit(StreamEvent::Stats {
                    packets_per_sec: (tile_packets + full_packets) as f64 / secs,
                    mbps: (tile_bytes + full_bytes) as f64 * 8.0 / secs / 1_000_000.0,
                    viewers,
                    skipped_frames: 0,
                    bitrate_mbps: mbps(active.adaptive.current()),
                    unstable: tile_lane.adaptive.is_reduced() || full_lane.adaptive.is_reduced(),
                });
            }
            report_start = Instant::now();
        }
    };

    capture.stop();
    drop(lanes); // libera os encoders nesta mesma thread
    result
}

fn camera_lost() -> BoxError {
    tr(
        "a câmera parou de enviar imagens (foi desconectada?)",
        "the camera stopped sending images (was it unplugged?)",
        "la cámara dejó de enviar imágenes (¿se desconectó?)",
    )
    .into()
}

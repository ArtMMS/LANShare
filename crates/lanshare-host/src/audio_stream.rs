//! Transmissão do som do Host: captura (process loopback) -> Opus -> todos os Clients.
//!
//! Roda numa thread própria, ao lado da transmissão da tela. Se o som falhar,
//! a imagem continua: o problema vira só um aviso.
//!
//! Monitor inteiro: o som vem do mixer (audio_mixer.rs), sem os apps bloqueados.
//! Janela: uma captura só, do app da janela.

use crate::audio_capture::{AudioCapture, AudioChunk, AudioSource, CHANNELS, SAMPLE_RATE};
use crate::audio_mixer::MixedCapture;
use crate::events::EventSender;
use crate::registry::Registry;
use lanshare_core::i18n::tr;
use opus::{Application, Bitrate, Channels, Encoder};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type BoxError = Box<dyn Error + Send + Sync>;

/// Cada pacote Opus leva 20 ms de som.
const FRAME_MS: u64 = 20;
const FRAME_SAMPLES_PER_CHANNEL: usize = (SAMPLE_RATE as u64 * FRAME_MS / 1000) as usize; // 960
const FRAME_SAMPLES: usize = FRAME_SAMPLES_PER_CHANNEL * CHANNELS as usize; // 1920

/// Qualidade do som: 128 kbps em estéreo é praticamente igual ao original.
const BITRATE_BPS: i32 = 128_000;

/// Maior pacote Opus possível (o encoder nunca passa disso).
const MAX_PACKET: usize = 4000;

/// Se o próximo bloco capturado chegar com esta diferença (ou mais) do esperado,
/// houve um buraco na captura (nada tocando): recomeça a contagem do tempo dali.
const GAP_TOLERANCE: Duration = Duration::from_millis(30);

/// O som da transmissão em andamento.
pub struct AudioStreamer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioStreamer {
    /// Começa a capturar e enviar o som. `started` é o início da transmissão (o mesmo do vídeo).
    /// Não trava: a captura é ligada dentro da thread.
    pub fn start(registry: Registry, started: Instant, events: EventSender, source: AudioSource) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        let thread = std::thread::Builder::new()
            .name("lanshare-audio-stream".to_string())
            .spawn(move || {
                if let Err(erro) = run(&registry, started, &thread_stop, &events, source) {
                    events.warn(format!(
                        "{}: {erro}",
                        tr(
                            "O som da transmissão parou",
                            "The stream audio stopped",
                            "El sonido de la transmisión se detuvo"
                        )
                    ));
                }
            })
            .ok();

        Self { stop, thread }
    }

    /// Para o som e espera a thread terminar.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for AudioStreamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(
    registry: &Registry,
    started: Instant,
    stop: &AtomicBool,
    events: &EventSender,
    source: AudioSource,
) -> Result<(), BoxError> {
    let capture = Capture::start(source)?;

    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio)?;
    encoder.set_bitrate(Bitrate::Bits(BITRATE_BPS))?;

    let what = match source {
        AudioSource::SystemExceptSelf => tr("som do PC", "PC audio", "sonido del PC"),
        AudioSource::SystemWithoutExcluded => tr(
            "som do PC, sem os apps bloqueados",
            "PC audio, without the blocked apps",
            "sonido del PC, sin las apps bloqueadas",
        ),
        AudioSource::ProcessTree(_) => tr(
            "só o som da janela",
            "only the window's audio",
            "solo el sonido de la ventana",
        ),
    };
    events.info(format!(
        "{}: {what} (Opus {} kbps)",
        tr(
            "Som da transmissão ativo",
            "Stream audio active",
            "Sonido de la transmisión activo"
        ),
        BITRATE_BPS / 1000
    ));

    let mut framer = Framer::default();
    let mut packet = vec![0u8; MAX_PACKET];

    while !stop.load(Ordering::Relaxed) {
        let chunk = match capture.chunks().recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => chunk,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break, // a captura caiu (o erro vem no stop)
        };

        // Sem ninguém assistindo, não gasta CPU com o Opus
        if registry.is_empty() {
            framer.reset();
            continue;
        }

        for (samples, captured_at) in framer.push(&chunk.samples, chunk.captured_at) {
            let len = encoder.encode(&samples, &mut packet)?;
            let timestamp_us = captured_at.saturating_duration_since(started).as_micros() as u64;
            registry.broadcast_audio(timestamp_us, Arc::new(packet[..len].to_vec()));
        }
    }

    capture.stop()
}

/// De onde vêm os blocos de som: uma captura só ou o mixer (vários apps somados).
enum Capture {
    Single(AudioCapture),
    Mixed(MixedCapture),
}

impl Capture {
    fn start(source: AudioSource) -> Result<Self, BoxError> {
        Ok(match source {
            AudioSource::SystemWithoutExcluded => Self::Mixed(MixedCapture::start()?),
            other => Self::Single(AudioCapture::start(other)?),
        })
    }

    fn chunks(&self) -> &Receiver<AudioChunk> {
        match self {
            Self::Single(capture) => &capture.chunks,
            Self::Mixed(mixer) => &mixer.chunks,
        }
    }

    fn stop(self) -> Result<(), BoxError> {
        match self {
            Self::Single(capture) => capture.stop(),
            Self::Mixed(mixer) => {
                mixer.stop();
                Ok(())
            }
        }
    }
}

/// Junta os blocos da captura (tamanhos variados) em quadros de 20 ms para o Opus,
/// guardando quando cada quadro tocou.
#[derive(Default)]
struct Framer {
    pending: Vec<i16>,
    /// Quando tocou a primeira amostra de `pending`.
    pending_start: Option<Instant>,
}

impl Framer {
    fn reset(&mut self) {
        self.pending.clear();
        self.pending_start = None;
    }

    /// Duração das amostras guardadas.
    fn pending_duration(&self) -> Duration {
        let frames = (self.pending.len() / CHANNELS as usize) as u64;
        Duration::from_micros(frames * 1_000_000 / SAMPLE_RATE as u64)
    }

    /// Acrescenta um bloco e devolve os quadros de 20 ms que ficaram completos.
    fn push(&mut self, samples: &[i16], captured_at: Instant) -> Vec<(Vec<i16>, Instant)> {
        let mut frames = Vec::new();

        // Buraco na captura (o Windows não entrega pacotes quando nada toca):
        // fecha o quadro incompleto com silêncio e recomeça o tempo a partir deste bloco
        if let Some(start) = self.pending_start {
            let expected = start + self.pending_duration();
            if captured_at > expected + GAP_TOLERANCE {
                if !self.pending.is_empty() {
                    self.pending.resize(FRAME_SAMPLES, 0);
                    frames.push((std::mem::take(&mut self.pending), start));
                }
                self.pending_start = None;
            }
        }

        if self.pending_start.is_none() {
            self.pending.clear();
            self.pending_start = Some(captured_at);
        }

        self.pending.extend_from_slice(samples);

        while self.pending.len() >= FRAME_SAMPLES {
            let start = self.pending_start.unwrap_or(captured_at);
            let rest = self.pending.split_off(FRAME_SAMPLES);
            frames.push((std::mem::replace(&mut self.pending, rest), start));
            self.pending_start = Some(start + Duration::from_millis(FRAME_MS));
        }

        frames
    }
}
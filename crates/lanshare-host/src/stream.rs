//! Transmissão da tela do Host: captura -> encoder de GPU -> todos os Clients.

use crate::capture::ScreenCapture;
use crate::hw_encoder::{HwEncoder, TARGET_BITRATE_BPS, TARGET_FPS};
use crate::registry::Registry;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type BoxError = Box<dyn Error + Send + Sync>;

/// De quanto em quanto tempo o Host mostra o resumo da transmissão.
const REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// A transmissão em andamento (roda numa thread própria).
pub struct StreamHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StreamHandle {
    /// Começa a capturar, comprimir e enviar a tela aos Clients.
    pub fn start(registry: Registry) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        // O encoder de GPU precisa nascer e morrer na mesma thread: tudo fica dentro de run()
        let thread = std::thread::spawn(move || {
            if let Err(erro) = run(registry, thread_stop) {
                println!("[host] A transmissao parou: {erro}");
                println!("[host] Digite 'share' para tentar de novo.");
            }
        });

        Self {
            stop,
            thread: Some(thread),
        }
    }

    /// false se a transmissão já terminou sozinha (por erro).
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Para a transmissão e espera a thread terminar.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(registry: Registry, stop: Arc<AtomicBool>) -> Result<(), BoxError> {
    let capture = ScreenCapture::start()?;
    let mut encoder: Option<HwEncoder> = None;

    let started = Instant::now();
    let mut report_start = Instant::now();
    let mut packets: u32 = 0;
    let mut bytes: u64 = 0;
    let mut skipped: u32 = 0;

    // Limite de FPS: a agenda diz quando o próximo frame pode entrar
    let frame_interval = Duration::from_secs_f64(1.0 / TARGET_FPS as f64);
    let mut next_frame_at = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        let frame = match capture.frames.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err("a captura de tela foi encerrada".into());
            }
        };

        // Sem ninguém assistindo, não gasta a GPU
        // (quem entrar depois pede um keyframe sozinho)
        if registry.is_empty() {
            continue;
        }

        // Chegou antes da hora: descarta (folga de 1/4 de intervalo para o jitter)
        let now = Instant::now();
        if now + frame_interval / 4 < next_frame_at {
            skipped += 1;
            continue;
        }
        next_frame_at = if now > next_frame_at + frame_interval {
            now + frame_interval // ficou muito para trás: recomeça a agenda daqui
        } else {
            next_frame_at + frame_interval
        };

        // O encoder nasce no primeiro frame, quando já sabemos a resolução
        if encoder.is_none() {
            encoder = Some(HwEncoder::new(
                frame.width,
                frame.height,
                TARGET_FPS,
                TARGET_BITRATE_BPS,
            )?);
            println!(
                "[host] Transmitindo a tela ({}x{}). Digite 'stop' para parar.",
                frame.width, frame.height
            );
        }
        let Some(encoder) = encoder.as_mut() else {
            continue;
        };

        // Alguém entrou ou ficou para trás: o próximo frame sai como keyframe
        if registry.take_keyframe_request() {
            encoder.force_keyframe();
        }

        for packet in encoder.encode(&frame)? {
            let keyframe = is_keyframe(&packet);
            let timestamp_us = started.elapsed().as_micros() as u64;

            packets += 1;
            bytes += packet.len() as u64;

            registry.broadcast_video(keyframe, timestamp_us, Arc::new(packet));
        }

        if report_start.elapsed() >= REPORT_INTERVAL {
            let secs = report_start.elapsed().as_secs_f64();
            println!(
                "[host] Transmitindo: {:.1} pacotes/s, {:.2} Mbps (por client), {} client(s), {} frame(s) pulado(s)",
                packets as f64 / secs,
                bytes as f64 * 8.0 / secs / 1_000_000.0,
                registry.list().len(),
                skipped
            );
            report_start = Instant::now();
            packets = 0;
            bytes = 0;
            skipped = 0;
        }
    }

    capture.stop()?;
    Ok(())
}

/// Esse pacote H.264 (formato Annex B) é um keyframe?
/// Procura as marcas de início 00 00 01 e olha o tipo de cada bloco (5 = quadro completo).
fn is_keyframe(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            if data[i + 3] & 0x1F == 5 {
                return true;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}
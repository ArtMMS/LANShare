//! Transmissão da tela do Host: captura -> encoder de GPU -> todos os Clients.
//! O som vai junto, numa thread própria (audio_stream.rs), com o mesmo relógio da imagem:
//! - monitor inteiro: o som do PC, menos o LANShare e os apps bloqueados nas Configurações;
//! - janela: só o som do app daquela janela;
//! - som desligado (botão 🔊 do seletor): nenhum som.
//! (As mensagens de debug "diag" são temporárias, para investigar o travamento ao reiniciar o `share`.)

use crate::app_windows;
use crate::audio_capture::AudioSource;
use crate::audio_stream::AudioStreamer;
use crate::capture::{CaptureTarget, CapturedFrame, ScreenCapture};
use crate::events::{EventSender, HostEvent};
use crate::hw_encoder::{bitrate_for, HwEncoder};
use crate::preview::PreviewSlot;
use crate::registry::Registry;
use lanshare_core::i18n::tr;
use rayon::prelude::*;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type BoxError = Box<dyn Error + Send + Sync>;

/// De quanto em quanto tempo o Host emite o resumo da transmissão.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Quantos horários de captura podem esperar o encoder devolver o pacote correspondente.
/// O encoder (em baixa latência) devolve um pacote por frame; isto só protege contra frames
/// que ele decidiu não codificar.
const MAX_PENDING_TIMESTAMPS: usize = 8;

/// Tempo máximo esperando a transmissão trocar de monitor/janela.
const SWITCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Opções escolhidas pelo Host antes de transmitir.
#[derive(Debug, Clone, Copy)]
pub struct StreamSettings {
    /// Altura da imagem transmitida, em pixels (720 ou 1080).
    pub height: u32,
    /// Quadros por segundo (15 ou 30).
    pub fps: u32,
}

/// Pedido de troca do que está sendo capturado, com a resposta de volta.
struct SwitchRequest {
    target: CaptureTarget,
    reply: SyncSender<Result<(), String>>,
}

/// O som da transmissão e o que decide de onde ele vem.
struct AudioState {
    /// O que está sendo transmitido agora (monitor ou janela).
    target: CaptureTarget,
    /// Botão 🔊 do seletor: false = transmissão sem som.
    enabled: bool,
    /// De onde o som está vindo agora (None = sem som).
    source: Option<AudioSource>,
    streamer: Option<AudioStreamer>,
}

/// A transmissão em andamento (roda numa thread própria).
pub struct StreamHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    events: EventSender,
    switch_tx: Sender<SwitchRequest>,
    registry: Registry,
    /// Início da transmissão: relógio comum da imagem e do som.
    started: Instant,
    audio: Mutex<AudioState>,
}

impl StreamHandle {
    /// Começa a capturar, comprimir e enviar um monitor ou uma janela aos Clients.
    /// `audio_enabled` = false transmite só a imagem.
    pub fn start_on(
        registry: Registry,
        settings: StreamSettings,
        events: EventSender,
        preview: PreviewSlot,
        target: CaptureTarget,
        audio_enabled: bool,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_events = events.clone();
        let thread_preview = preview.clone();
        let thread_registry = registry.clone();
        let (switch_tx, switch_rx) = mpsc::channel();

        // Relógio comum da imagem e do som (os timestamps dos dois contam a partir daqui)
        let started = Instant::now();

        // O encoder de GPU precisa nascer e morrer na mesma thread: tudo fica dentro de run()
        let thread = std::thread::spawn(move || {
            let result = run(
                thread_registry,
                thread_stop,
                started,
                settings,
                target,
                switch_rx,
                &thread_events,
                &preview,
            );
            thread_preview.clear(); // a miniatura some junto com a transmissão
            thread_events.emit(HostEvent::StreamStopped {
                error: result.err().map(|erro| erro.to_string()),
            });
        });

        let handle = Self {
            stop,
            thread: Some(thread),
            events,
            switch_tx,
            registry,
            started,
            audio: Mutex::new(AudioState {
                target,
                enabled: audio_enabled,
                source: None,
                streamer: None,
            }),
        };
        handle.refresh_audio(&mut handle.lock_audio());
        handle
    }

    /// Troca o que está sendo transmitido sem parar a transmissão.
    /// Espera a troca acontecer e devolve o resultado. O som acompanha a troca.
    pub fn switch_source(&self, target: CaptureTarget) -> Result<(), String> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.switch_tx
            .send(SwitchRequest { target, reply })
            .map_err(|_| {
                tr(
                    "A transmissão já terminou.",
                    "The stream has already ended.",
                    "La transmisión ya terminó.",
                )
                .to_string()
            })?;

        answer.recv_timeout(SWITCH_TIMEOUT).map_err(|_| {
            tr(
                "A troca demorou demais. Tente de novo.",
                "The switch took too long. Try again.",
                "El cambio tardó demasiado. Inténtalo de nuevo.",
            )
            .to_string()
        })??;

        let mut audio = self.lock_audio();
        audio.target = target;
        self.refresh_audio(&mut audio);
        Ok(())
    }

    /// Liga ou desliga o som da transmissão em andamento (botão 🔊).
    pub fn set_audio_enabled(&self, enabled: bool) {
        let mut audio = self.lock_audio();
        audio.enabled = enabled;
        self.refresh_audio(&mut audio);
    }

    /// false se a transmissão já terminou sozinha (por erro).
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Para a transmissão e espera a thread terminar.
    pub fn stop(mut self) {
        self.events.debug("stop: pedindo para a thread parar");
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.events.debug("stop: a thread terminou");

        let streamer = self
            .audio
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .streamer
            .take();
        if let Some(streamer) = streamer {
            streamer.stop();
        }
    }

    /// Confere de onde o som deve vir agora e, se mudou, troca a captura do som.
    /// O relógio continua o mesmo: o som novo segue sincronizado com a imagem.
    fn refresh_audio(&self, audio: &mut AudioState) {
        let wanted = audio_source(audio.target, audio.enabled, &self.events);
        if wanted == audio.source && (audio.streamer.is_some() || wanted.is_none()) {
            return;
        }

        if let Some(old) = audio.streamer.take() {
            old.stop();
        }

        audio.source = wanted;
        audio.streamer = wanted.map(|source| {
            AudioStreamer::start(self.registry.clone(), self.started, self.events.clone(), source)
        });

        if wanted.is_none() && !audio.enabled {
            self.events.info(tr(
                "Transmissão sem som.",
                "Streaming without audio.",
                "Transmisión sin sonido.",
            ));
        }
    }

    fn lock_audio(&self) -> MutexGuard<'_, AudioState> {
        self.audio.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// De onde vem o som de cada tipo de transmissão (None = sem som).
fn audio_source(target: CaptureTarget, enabled: bool, events: &EventSender) -> Option<AudioSource> {
    if !enabled {
        return None;
    }

    match target {
        // Janela: só o som do app dela (e dos processos filhos)
        CaptureTarget::Window(id) => match app_windows::window_process_id(id) {
            Some(pid) => Some(AudioSource::ProcessTree(pid)),
            None => {
                // Sem saber o app, não arrisca mandar o som do PC inteiro (pode ter uma chamada)
                events.warn(tr(
                    "Não foi possível descobrir o app da janela: a transmissão segue sem som.",
                    "Could not find the window's app: the stream continues without audio.",
                    "No se pudo identificar la app de la ventana: la transmisión sigue sin sonido.",
                ));
                None
            }
        },
        // Monitor inteiro: o som do PC, menos o LANShare e os apps bloqueados (Discord, Teams...)
        CaptureTarget::PrimaryMonitor | CaptureTarget::Monitor(_) => Some(AudioSource::SystemWithoutExcluded),
    }
}

/// Tamanho fixo da imagem transmitida: 16:9 na altura escolhida (1280x720 ou 1920x1080).
/// Qualquer fonte (monitor ou janela, de qualquer tamanho) é encaixada nele.
fn canvas_size(height: u32) -> (u32, u32) {
    let height = height & !1;
    ((height * 16 / 9) & !1, height)
}

fn run(
    registry: Registry,
    stop: Arc<AtomicBool>,
    started: Instant,
    settings: StreamSettings,
    target: CaptureTarget,
    switch_rx: Receiver<SwitchRequest>,
    events: &EventSender,
    preview: &PreviewSlot,
) -> Result<(), BoxError> {
    events.debug("run: iniciando a captura");
    let mut capture = ScreenCapture::start_target(target)?;
    let mut current = target;
    events.debug("run: captura iniciada");
    let mut encoder: Option<HwEncoder> = None;

    let (canvas_w, canvas_h) = canvas_size(settings.height);

    let mut report_start = Instant::now();
    let mut packets: u32 = 0;
    let mut bytes: u64 = 0;
    let mut skipped: u32 = 0;
    let mut first_frame_logged = false;
    let mut first_packet_logged = false;
    let mut last_preview: Option<Instant> = None;

    // Quando cada frame entregue ao encoder foi capturado (o pacote leva esse horário,
    // não o do envio: é o que deixa o Client sincronizar a imagem com o som)
    let mut pending_timestamps: VecDeque<u64> = VecDeque::new();

    // Limite de FPS: a agenda diz quando o próximo frame pode entrar
    let frame_interval = Duration::from_secs_f64(1.0 / settings.fps as f64);
    let mut next_frame_at = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        // Troca de monitor/janela: liga a captura nova antes de desligar a antiga
        if let Ok(request) = switch_rx.try_recv() {
            let result = match ScreenCapture::start_target(request.target) {
                Ok(new_capture) => {
                    let old = std::mem::replace(&mut capture, new_capture);
                    let _ = old.stop();
                    current = request.target;
                    // A imagem nova aparece na hora para quem está assistindo
                    if let Some(encoder) = encoder.as_ref() {
                        encoder.force_keyframe();
                    }
                    Ok(())
                }
                Err(erro) => Err(format!(
                    "{}: {erro}",
                    tr(
                        "Não foi possível trocar a transmissão",
                        "Could not switch the stream",
                        "No se pudo cambiar la transmisión"
                    )
                )),
            };
            let _ = request.reply.send(result);
        }

        let frame = match capture.frames.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                let reason = match current {
                    CaptureTarget::Window(_) => tr(
                        "a janela compartilhada foi fechada",
                        "the shared window was closed",
                        "la ventana compartida se cerró",
                    ),
                    _ => tr(
                        "a captura de tela foi encerrada",
                        "the screen capture ended",
                        "la captura de pantalla terminó",
                    ),
                };
                return Err(reason.into());
            }
        };

        if !first_frame_logged {
            first_frame_logged = true;
            events.debug("run: primeiro frame recebido da captura");
        }

        // Encaixa no quadro fixo (se já não tiver o tamanho certo)
        let frame = if frame.width == canvas_w && frame.height == canvas_h {
            frame
        } else {
            fit_into(&frame, canvas_w, canvas_h)
        };

        // Preview do Host: no mesmo FPS da transmissão (com folga de 1/4 para o jitter)
        let want_preview =
            last_preview.map_or(true, |t| t.elapsed() + frame_interval / 4 >= frame_interval);

        // Sem ninguém assistindo, não gasta a GPU
        // (quem entrar depois pede um keyframe sozinho)
        if registry.is_empty() {
            if want_preview {
                preview.store(frame, settings.height);
                last_preview = Some(Instant::now());
            }
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

        // O encoder nasce no primeiro frame enviado
        if encoder.is_none() {
            events.debug("run: criando o encoder");
            let created = HwEncoder::new(
                frame.width,
                frame.height,
                settings.height,
                settings.fps,
                bitrate_for(settings.height, settings.fps),
            )?;
            events.debug("run: encoder criado");
            let (out_w, out_h) = created.frame_size();
            events.emit(HostEvent::StreamStarted {
                source_width: frame.width,
                source_height: frame.height,
                width: out_w,
                height: out_h,
                fps: settings.fps,
            });
            encoder = Some(created);
        }
        let Some(encoder) = encoder.as_mut() else {
            continue;
        };

        // Alguém entrou ou ficou para trás: o próximo frame sai como keyframe
        if registry.take_keyframe_request() {
            encoder.force_keyframe();
        }

        let captured_us = frame.captured_at.saturating_duration_since(started).as_micros() as u64;
        pending_timestamps.push_back(captured_us);
        while pending_timestamps.len() > MAX_PENDING_TIMESTAMPS {
            pending_timestamps.pop_front();
        }

        for packet in encoder.encode(&frame)? {
            if !first_packet_logged {
                first_packet_logged = true;
                events.debug(format!(
                    "run: primeiro pacote codificado ({} bytes)",
                    packet.len()
                ));
            }

            let keyframe = is_keyframe(&packet);
            // O pacote corresponde ao frame mais antigo ainda sem pacote
            let timestamp_us = pending_timestamps.pop_front().unwrap_or(captured_us);

            packets += 1;
            bytes += packet.len() as u64;

            registry.broadcast_video(keyframe, timestamp_us, Arc::new(packet));
        }

        // O encoder já terminou de usar o frame: entrega ao preview
        if want_preview {
            preview.store(frame, settings.height);
            last_preview = Some(Instant::now());
        }

        if report_start.elapsed() >= REPORT_INTERVAL {
            let secs = report_start.elapsed().as_secs_f64();
            events.emit(HostEvent::StreamStats {
                packets_per_sec: packets as f64 / secs,
                mbps: bytes as f64 * 8.0 / secs / 1_000_000.0,
                clients: registry.list().len(),
                skipped_frames: skipped,
            });
            report_start = Instant::now();
            packets = 0;
            bytes = 0;
            skipped = 0;
        }
    }

    events.debug("run: o laco terminou, parando a captura");
    capture.stop()?;
    events.debug("run: captura parada, liberando o encoder");
    drop(encoder);
    events.debug("run: encoder liberado");
    Ok(())
}

/// Encaixa o frame num quadro de `width` x `height`, mantendo a proporção
/// (centralizado, com faixas pretas onde sobrar).
fn fit_into(frame: &CapturedFrame, width: u32, height: u32) -> CapturedFrame {
    let (sw, sh) = (frame.width as usize, frame.height as usize);
    let (cw, ch) = (width as usize, height as usize);
    let mut data = vec![0u8; cw * ch * 4];

    if sw == 0 || sh == 0 || frame.data.len() < sw * sh * 4 {
        for px in data.chunks_exact_mut(4) {
            px[3] = 255;
        }
        return CapturedFrame {
            width,
            height,
            data,
            captured_at: frame.captured_at,
        };
    }

    let scale = (cw as f64 / sw as f64).min(ch as f64 / sh as f64);
    let dw = ((sw as f64 * scale).round() as usize).clamp(1, cw);
    let dh = ((sh as f64 * scale).round() as usize).clamp(1, ch);
    let (x0, y0) = ((cw - dw) / 2, (ch - dh) / 2);
    let src = &frame.data;

    data.par_chunks_mut(cw * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for px in row.chunks_exact_mut(4) {
                px[3] = 255; // fundo preto opaco
            }
            if y < y0 || y >= y0 + dh {
                return;
            }

            // Cada pixel novo é a média dos pixels originais que ele cobre
            let dy = y - y0;
            let sy0 = dy * sh / dh;
            let sy1 = ((dy + 1) * sh / dh).max(sy0 + 1).min(sh);

            for dx in 0..dw {
                let sx0 = dx * sw / dw;
                let sx1 = ((dx + 1) * sw / dw).max(sx0 + 1).min(sw);

                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for sy in sy0..sy1 {
                    for px in src[(sy * sw + sx0) * 4..(sy * sw + sx1) * 4].chunks_exact(4) {
                        r += px[0] as u32;
                        g += px[1] as u32;
                        b += px[2] as u32;
                    }
                }

                let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
                let o = (x0 + dx) * 4;
                row[o] = (r / n) as u8;
                row[o + 1] = (g / n) as u8;
                row[o + 2] = (b / n) as u8;
            }
        });

    CapturedFrame {
        width,
        height,
        data,
        captured_at: frame.captured_at,
    }
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
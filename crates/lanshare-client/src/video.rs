//! Vídeo do Client dentro do app: decodifica o H.264 numa thread própria
//! e guarda a imagem mais recente para a interface buscar.
//!
//! Sincronia: cada imagem decodificada espera o som chegar no momento em que ela foi
//! capturada no Host (sync_clock.rs). Sem som tocando, ela aparece assim que fica pronta.

use crate::decoder::{Decoder, Picture};
use crate::engine::EventTx;
use crate::sync_clock::SyncClock;
use lanshare_core::i18n::tr;
use lanshare_core::protocol::VideoFrame;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Quantos pacotes podem esperar na fila antes de a decodificação "ficar para trás".
const QUEUE_SIZE: usize = 16;

/// Uma imagem pode aparecer até este tanto antes do som (abaixo de ~20 ms ninguém percebe).
const SYNC_LEAD_US: u64 = 10_000;

/// Maior espera de uma imagem pelo som. Se o som estiver mais atrasado que isso,
/// a imagem aparece mesmo assim (melhor fora de sincronia do que travada).
const MAX_HOLD: Duration = Duration::from_millis(350);

/// Limite de imagens esperando o som (cada uma ocupa ~3 MB em 1080p).
const MAX_HELD: usize = 12;

/// De quanto em quanto tempo a thread confere se já é hora de mostrar a próxima imagem.
const RELEASE_POLL: Duration = Duration::from_millis(4);

/// Sem nada esperando, a thread só acorda quando chega pacote (ou para conferir o fim).
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// De quanto em quanto tempo o motor avisa a diferença entre imagem e som.
const SYNC_REPORT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
struct SlotData {
    /// Muda a cada conexão: imagens de uma conexão antiga são ignoradas.
    generation: u64,
    width: usize,
    height: usize,
    /// Pixels 0x00RRGGBB (na memória: B, G, R, 0 = BGRA).
    pixels: Vec<u32>,
    seq: u64,
    read_seq: u64,
}

impl SlotData {
    fn reset(&mut self) {
        self.width = 0;
        self.height = 0;
        self.pixels.clear();
        self.seq = 0;
        self.read_seq = 0;
    }
}

/// Guarda a imagem mais recente do vídeo recebido.
#[derive(Clone, Default)]
pub struct FrameSlot {
    inner: Arc<Mutex<SlotData>>,
}

impl FrameSlot {
    fn lock(&self) -> MutexGuard<'_, SlotData> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Começa uma conexão nova: limpa a imagem e devolve o número da conexão.
    pub fn begin_session(&self) -> u64 {
        let mut data = self.lock();
        data.generation += 1;
        data.reset();
        data.generation
    }

    /// Fim da conexão: limpa a imagem e invalida quem ainda estiver escrevendo.
    pub fn end_session(&self) {
        let mut data = self.lock();
        data.generation += 1;
        data.reset();
    }

    /// Guarda uma imagem nova (troca os buffers, sem copiar).
    fn publish(&self, generation: u64, width: usize, height: usize, pixels: &mut Vec<u32>) {
        let mut data = self.lock();
        if data.generation != generation {
            return;
        }
        std::mem::swap(&mut data.pixels, pixels);
        data.width = width;
        data.height = height;
        data.seq += 1;
    }

    /// Copia a imagem (BGRA) para `out`, só se for nova desde a última leitura.
    pub fn read_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        let mut data = self.lock();
        if data.seq == data.read_seq || data.pixels.is_empty() {
            return None;
        }

        let pixel_count = data.width * data.height;
        let needed = pixel_count * 4;
        if needed == 0 || data.pixels.len() < pixel_count || out.len() < needed {
            return None;
        }

        // SAFETY: `pixels` tem pelo menos pixel_count u32 (= needed bytes) e u8 não exige alinhamento
        let bytes = unsafe { std::slice::from_raw_parts(data.pixels.as_ptr() as *const u8, needed) };
        out[..needed].copy_from_slice(bytes);

        data.read_seq = data.seq;
        Some((data.width as u32, data.height as u32))
    }
}

/// A thread que decodifica o vídeo. Ao ser descartada, a thread termina sozinha.
pub struct VideoPipeline {
    tx: SyncSender<VideoFrame>,
    /// Ligado quando um pacote foi descartado (fila cheia).
    gap: Arc<AtomicBool>,
}

impl VideoPipeline {
    /// `clock` é o relógio do som desta transmissão (o mesmo do AudioPlayer).
    pub fn start(slot: FrameSlot, generation: u64, events: EventTx, clock: SyncClock) -> Self {
        let (tx, rx) = sync_channel(QUEUE_SIZE);
        let gap = Arc::new(AtomicBool::new(false));
        let thread_gap = gap.clone();

        // O decodificador precisa ser criado e usado na mesma thread
        let _ = std::thread::Builder::new()
            .name("lanshare-decoder".to_string())
            .spawn(move || run(rx, thread_gap, slot, generation, events, clock));

        Self { tx, gap }
    }

    /// Entrega um pacote. Nunca trava a conexão: se a fila estiver cheia, descarta.
    pub fn push(&self, frame: VideoFrame) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(frame) {
            self.gap.store(true, Ordering::Relaxed);
        }
    }
}

/// Decodificador + regras de quando decodificar.
struct Pipeline {
    decoder: Decoder,
    gap: Arc<AtomicBool>,
    /// true = descartando pacotes até chegar um keyframe (nada decodifica sem ele).
    waiting_keyframe: bool,
}

impl Pipeline {
    /// Decodifica um pacote e devolve a imagem mais recente que saiu dele (se saiu alguma),
    /// com o momento em que ela foi capturada no Host.
    fn process(&mut self, frame: VideoFrame) -> Option<Held> {
        // Perdemos pacotes: os próximos dependem deles, então espera o próximo keyframe
        if self.gap.swap(false, Ordering::Relaxed) {
            self.wait_for_keyframe();
        }

        if self.waiting_keyframe {
            if !frame.keyframe {
                return None;
            }
            self.waiting_keyframe = false;
        }

        match self.decoder.decode(&frame.data, frame.timestamp_us) {
            Ok(mut pictures) => pictures.pop().map(|picture| Held {
                timestamp_us: frame.timestamp_us,
                since: Instant::now(),
                picture,
            }),
            Err(erro) => {
                println!("[client] Erro ao decodificar: {erro}");
                self.wait_for_keyframe();
                None
            }
        }
    }

    fn wait_for_keyframe(&mut self) {
        self.waiting_keyframe = true;
        self.decoder.flush();
    }
}

/// Uma imagem decodificada esperando a hora de aparecer.
struct Held {
    /// Quando ela foi capturada no Host (µs desde o início da transmissão).
    timestamp_us: u64,
    /// Desde quando ela está esperando.
    since: Instant,
    picture: Picture,
}

/// Imagens decodificadas esperando o som alcançá-las.
#[derive(Default)]
struct SyncQueue {
    held: VecDeque<Held>,
}

impl SyncQueue {
    fn push(&mut self, held: Held) {
        self.held.push_back(held);
    }

    /// A imagem que deve aparecer agora (a mais nova que já está na hora).
    /// As mais velhas que também já estavam na hora são puladas.
    /// Devolve também a diferença entre a imagem e o som, quando há som.
    fn release(&mut self, clock: &SyncClock) -> Option<(Picture, Option<i64>)> {
        let audio_us = clock.now_us();
        let mut chosen = None;

        while let Some(front) = self.held.front() {
            let due = match audio_us {
                None => true, // sem som: aparece já
                Some(audio) => front.timestamp_us <= audio + SYNC_LEAD_US,
            } || front.since.elapsed() >= MAX_HOLD
                || self.held.len() > MAX_HELD;

            if !due {
                break;
            }

            let Some(held) = self.held.pop_front() else { break };
            // Positivo = a imagem apareceu antes do som; negativo = depois
            let offset = audio_us.map(|audio| held.timestamp_us as i64 - audio as i64);
            chosen = Some((held.picture, offset));
        }

        chosen
    }

    fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// Média da diferença entre imagem e som, para o evento "sync_stats"
/// (null quando nenhuma imagem do período teve som para acompanhar).
struct SyncStats {
    since: Instant,
    total_us: i64,
    count: i64,
}

impl SyncStats {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            total_us: 0,
            count: 0,
        }
    }

    fn record(&mut self, offset_us: Option<i64>, events: &EventTx) {
        if let Some(offset) = offset_us {
            self.total_us += offset;
            self.count += 1;
        }

        if self.since.elapsed() >= SYNC_REPORT_INTERVAL {
            let offset_ms =
                (self.count > 0).then(|| self.total_us as f64 / self.count as f64 / 1000.0);
            let _ = events.send(json!({ "type": "sync_stats", "offset_ms": offset_ms }));
            *self = Self::new();
        }
    }
}

fn run(
    rx: Receiver<VideoFrame>,
    gap: Arc<AtomicBool>,
    slot: FrameSlot,
    generation: u64,
    events: EventTx,
    clock: SyncClock,
) {
    let decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        Err(erro) => {
            let _ = events.send(json!({
                "type": "video_error",
                "error": format!(
                    "{}: {erro}",
                    tr(
                        "Não foi possível iniciar o decodificador de vídeo",
                        "Could not start the video decoder",
                        "No se pudo iniciar el decodificador de vídeo"
                    )
                ),
            }));
            return;
        }
    };

    let mut pipeline = Pipeline {
        decoder,
        gap,
        waiting_keyframe: true,
    };

    let mut queue = SyncQueue::default();
    let mut stats = SyncStats::new();
    let mut rgb: Vec<u32> = Vec::new();
    let mut size = (0usize, 0usize);

    loop {
        // Com imagens esperando, acorda logo para conferir a hora delas
        let wait = if queue.is_empty() { IDLE_WAIT } else { RELEASE_POLL };

        match rx.recv_timeout(wait) {
            Ok(frame) => {
                if let Some(held) = pipeline.process(frame) {
                    queue.push(held);
                }
                // Decodifica tudo o que já chegou
                while let Ok(frame) = rx.try_recv() {
                    if let Some(held) = pipeline.process(frame) {
                        queue.push(held);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // A conexão acabou e o VideoPipeline foi descartado
            Err(RecvTimeoutError::Disconnected) => return,
        }

        let Some((picture, offset)) = queue.release(&clock) else {
            continue;
        };
        stats.record(offset, &events);

        picture.to_rgb(&mut rgb);

        let new_size = (picture.width, picture.height);
        if new_size != size {
            size = new_size;
            let _ = events.send(json!({
                "type": "video_started",
                "width": new_size.0,
                "height": new_size.1,
            }));
        }

        slot.publish(generation, new_size.0, new_size.1, &mut rgb);
    }
}

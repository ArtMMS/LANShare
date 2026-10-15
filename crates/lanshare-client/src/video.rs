//! Vídeo do Client dentro do app: decodifica o H.264 numa thread própria
//! e guarda a imagem mais recente para a interface buscar.

use crate::decoder::{Decoder, Picture};
use crate::engine::EventTx;
use lanshare_core::protocol::VideoFrame;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};

/// Quantos pacotes podem esperar na fila antes de a decodificação "ficar para trás".
const QUEUE_SIZE: usize = 16;

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
    pub fn start(slot: FrameSlot, generation: u64, events: EventTx) -> Self {
        let (tx, rx) = sync_channel(QUEUE_SIZE);
        let gap = Arc::new(AtomicBool::new(false));
        let thread_gap = gap.clone();

        // O decodificador precisa ser criado e usado na mesma thread
        let _ = std::thread::Builder::new()
            .name("lanshare-decoder".to_string())
            .spawn(move || run(rx, thread_gap, slot, generation, events));

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
    /// Decodifica um pacote e devolve a imagem mais recente que saiu dele (se saiu alguma).
    fn process(&mut self, frame: VideoFrame) -> Option<Picture> {
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
            Ok(mut pictures) => pictures.pop(),
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

fn run(
    rx: Receiver<VideoFrame>,
    gap: Arc<AtomicBool>,
    slot: FrameSlot,
    generation: u64,
    events: EventTx,
) {
    let decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        Err(erro) => {
            let _ = events.send(json!({
                "type": "video_error",
                "error": format!("Não foi possível iniciar o decodificador de vídeo: {erro}"),
            }));
            return;
        }
    };

    let mut pipeline = Pipeline {
        decoder,
        gap,
        waiting_keyframe: true,
    };

    let mut rgb: Vec<u32> = Vec::new();
    let mut size = (0usize, 0usize);

    // recv() termina quando a conexão acaba e o VideoPipeline é descartado
    while let Ok(frame) = rx.recv() {
        let mut newest = pipeline.process(frame);

        // Decodifica tudo o que já chegou, mas só a última imagem é guardada
        while let Ok(frame) = rx.try_recv() {
            if let Some(picture) = pipeline.process(frame) {
                newest = Some(picture);
            }
        }

        if let Some(picture) = newest {
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
}
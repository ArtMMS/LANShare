//! Janela provisória do Client: recebe os pacotes de vídeo, decodifica e mostra na tela.
//! Roda numa thread própria (decodificador e janela precisam ficar na mesma thread).
//! Quando a GUI em C# chegar, só a janela daqui é trocada; o decoder.rs continua valendo.

use crate::decoder::{Decoder, Picture};
use lanshare_core::protocol::VideoFrame;
use minifb::{Key, ScaleMode, Window, WindowOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Quantos pacotes podem esperar na fila antes de a janela "ficar para trás".
const QUEUE_SIZE: usize = 16;

/// Tamanho máximo da janela ao abrir (a imagem é reduzida mantendo a proporção).
const MAX_WINDOW_WIDTH: f32 = 1280.0;
const MAX_WINDOW_HEIGHT: f32 = 720.0;

/// A janela de vídeo (e a thread que a mantém).
pub struct Viewer {
    tx: Option<SyncSender<VideoFrame>>,
    /// Ligado quando um pacote foi descartado (fila cheia).
    gap: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Viewer {
    /// `on_close` é chamada quando o usuário fecha a janela (ou aperta Esc).
    pub fn start(on_close: impl FnOnce() + Send + 'static) -> Self {
        let (tx, rx) = sync_channel(QUEUE_SIZE);
        let gap = Arc::new(AtomicBool::new(false));
        let thread_gap = gap.clone();

        let thread = std::thread::spawn(move || run(rx, thread_gap, on_close));

        Self {
            tx: Some(tx),
            gap,
            thread: Some(thread),
        }
    }

    /// Entrega um pacote de vídeo à janela. Nunca trava a conexão: se a fila estiver cheia, descarta.
    pub fn push(&self, frame: VideoFrame) {
        let Some(tx) = &self.tx else { return };

        if let Err(TrySendError::Full(_)) = tx.try_send(frame) {
            self.gap.store(true, Ordering::Relaxed);
        }
    }

    /// Fecha a janela e espera a thread terminar.
    pub fn stop(mut self) {
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Decodificador + regras de quando decodificar.
struct Pipeline {
    decoder: Decoder,
    gap: Arc<AtomicBool>,
    /// true = descartando pacotes até chegar um keyframe (nada decodifica sem ele).
    waiting_keyframe: bool,
    warned_sps: bool,
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

            // O keyframe precisa trazer o SPS/PPS, senão o decodificador não sabe o tamanho
            if !self.warned_sps && !has_sps(&frame.data) {
                self.warned_sps = true;
                println!("[client] Aviso: o keyframe veio sem SPS/PPS; o decodificador pode nao iniciar.");
            }
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

fn run(rx: Receiver<VideoFrame>, gap: Arc<AtomicBool>, on_close: impl FnOnce()) {
    let decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        Err(erro) => {
            println!("[client] Nao foi possivel iniciar o decodificador de video: {erro}");
            return;
        }
    };

    let mut pipeline = Pipeline {
        decoder,
        gap,
        waiting_keyframe: true,
        warned_sps: false,
    };

    let mut window: Option<Window> = None;
    let mut rgb: Vec<u32> = Vec::new();

    loop {
        // Espera um pacote (pouco tempo, para a janela continuar respondendo)
        let mut newest: Option<Picture> = None;
        match rx.recv_timeout(Duration::from_millis(15)) {
            Ok(frame) => {
                newest = pipeline.process(frame);

                // Decodifica tudo o que já chegou, mas só a última imagem vai para a tela
                while let Ok(frame) = rx.try_recv() {
                    if let Some(picture) = pipeline.process(frame) {
                        newest = Some(picture);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if let Some(picture) = newest {
            picture.to_rgb(&mut rgb);

            // A janela nasce na primeira imagem, quando já sabemos o tamanho
            if window.is_none() {
                match open_window(picture.width, picture.height) {
                    Ok(opened) => window = Some(opened),
                    Err(erro) => {
                        println!("[client] Nao foi possivel abrir a janela: {erro}");
                        break;
                    }
                }
            }

            if let Some(window) = window.as_mut() {
                if let Err(erro) = window.update_with_buffer(&rgb, picture.width, picture.height) {
                    println!("[client] Erro ao desenhar o video: {erro}");
                    break;
                }
            }
        } else if let Some(window) = window.as_mut() {
            window.update();
        }

        if let Some(window) = window.as_ref() {
            if !window.is_open() || window.is_key_down(Key::Escape) {
                on_close();
                break;
            }
        }
    }
}

fn open_window(width: usize, height: usize) -> Result<Window, minifb::Error> {
    let scale = (MAX_WINDOW_WIDTH / width as f32)
        .min(MAX_WINDOW_HEIGHT / height as f32)
        .min(1.0);
    let window_w = ((width as f32 * scale) as usize).max(160);
    let window_h = ((height as f32 * scale) as usize).max(90);

    Window::new(
        "LANShare",
        window_w,
        window_h,
        WindowOptions {
            resize: true,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
}

/// O pacote H.264 (Annex B) tem um bloco SPS (tipo 7)?
fn has_sps(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            if data[i + 3] & 0x1F == 7 {
                return true;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}
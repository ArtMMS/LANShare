//! Preview do Host: mostra na janela a mesma imagem que é transmitida.
//!
//! A thread de transmissão entrega o frame capturado (sem copiar), no mesmo ritmo (FPS) da
//! transmissão, e a interface lê a imagem já no tamanho enviado aos Clients (720p ou 1080p),
//! convertida para BGRA.
//! Guarda só um frame, então nunca acumula memória.

use crate::capture::CapturedFrame;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

/// Maior imagem que o preview pode devolver (a interface reserva esse espaço).
pub const PREVIEW_MAX_WIDTH: usize = 2560;
pub const PREVIEW_MAX_HEIGHT: usize = 1440;

/// O frame mais recente e a altura que a transmissão usa (o preview tem a mesma).
struct Pending {
    frame: CapturedFrame,
    target_height: u32,
}

/// Caixa compartilhada entre a transmissão (que escreve) e a interface (que lê).
#[derive(Clone, Default)]
pub struct PreviewSlot {
    inner: Arc<Mutex<Option<Pending>>>,
}

impl PreviewSlot {
    /// Guarda o frame (sem copiar) como o mais recente. `target_height` = altura da transmissão.
    pub fn store(&self, frame: CapturedFrame, target_height: u32) {
        *self.lock() = Some(Pending {
            frame,
            target_height,
        });
    }

    /// Apaga o frame guardado (a transmissão terminou).
    pub fn clear(&self) {
        *self.lock() = None;
    }

    /// Converte o frame novo para BGRA no tamanho da transmissão e grava em `out`.
    /// Devolve (largura, altura), ou None se não há frame novo desde a última leitura
    /// (a interface continua mostrando a imagem anterior) ou se `out` é pequeno demais.
    pub fn read_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        // Tira o frame da caixa: a conversão roda fora do lock, sem travar a transmissão
        let pending = self.lock().take()?;
        let frame = &pending.frame;

        let (dst_w, dst_h) = output_size(frame, pending.target_height)?;
        let needed = dst_w * dst_h * 4;
        if out.len() < needed {
            return None;
        }

        // Cada tarefa converte uma linha; usa todos os núcleos
        out[..needed]
            .par_chunks_mut(dst_w * 4)
            .enumerate()
            .for_each(|(dy, row)| convert_row(frame, dy, row, dst_w, dst_h));

        Some((dst_w as u32, dst_h as u32))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Pending>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Tamanho do preview: a altura da transmissão, na proporção da tela. Nunca amplia.
fn output_size(frame: &CapturedFrame, target_height: u32) -> Option<(usize, usize)> {
    let (src_w, src_h) = (frame.width as usize, frame.height as usize);
    if src_w == 0 || src_h == 0 || frame.data.len() < src_w * src_h * 4 {
        return None;
    }

    let scale = (target_height as f64 / src_h as f64)
        .min(PREVIEW_MAX_WIDTH as f64 / src_w as f64)
        .min(PREVIEW_MAX_HEIGHT as f64 / src_h as f64)
        .min(1.0);
    let dst_w = ((src_w as f64 * scale) as usize).clamp(2, PREVIEW_MAX_WIDTH);
    let dst_h = ((src_h as f64 * scale) as usize).clamp(2, PREVIEW_MAX_HEIGHT);
    Some((dst_w, dst_h))
}

/// Converte uma linha RGBA -> BGRA. Se a imagem foi reduzida, cada pixel novo é a média
/// de 4 pixels do bloco original (o mesmo tipo de redução que o encoder usa).
fn convert_row(frame: &CapturedFrame, dy: usize, row: &mut [u8], dst_w: usize, dst_h: usize) {
    let (src_w, src_h) = (frame.width as usize, frame.height as usize);
    let src = &frame.data;

    // Mesmo tamanho: só troca a ordem dos canais
    if dst_w == src_w && dst_h == src_h {
        let line = &src[dy * src_w * 4..(dy + 1) * src_w * 4];
        for (out, px) in row.chunks_exact_mut(4).zip(line.chunks_exact(4)) {
            out[0] = px[2];
            out[1] = px[1];
            out[2] = px[0];
            out[3] = 255;
        }
        return;
    }

    let sy0 = (dy * src_h / dst_h).min(src_h - 1);
    let sy1 = (((dy + 1) * src_h / dst_h).saturating_sub(1)).clamp(sy0, src_h - 1);

    for (dx, out) in row.chunks_exact_mut(4).enumerate() {
        let sx0 = (dx * src_w / dst_w).min(src_w - 1);
        let sx1 = (((dx + 1) * src_w / dst_w).saturating_sub(1)).clamp(sx0, src_w - 1);

        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        for (x, y) in [(sx0, sy0), (sx1, sy0), (sx0, sy1), (sx1, sy1)] {
            let s = (y * src_w + x) * 4;
            r += src[s] as u32;
            g += src[s + 1] as u32;
            b += src[s + 2] as u32;
        }

        out[0] = (b / 4) as u8;
        out[1] = (g / 4) as u8;
        out[2] = (r / 4) as u8;
        out[3] = 255;
    }
}
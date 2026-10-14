//! Preview do Host: uma miniatura do que está sendo transmitido.
//!
//! A thread de transmissão publica uma miniatura a cada ~100 ms; a interface
//! (via FFI) copia a última publicada. Guarda só um quadro, então nunca acumula memória.

use crate::capture::CapturedFrame;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Tamanho máximo da miniatura (a imagem cabe nessa caixa, mantendo a proporção da tela).
pub const PREVIEW_MAX_WIDTH: usize = 960;
pub const PREVIEW_MAX_HEIGHT: usize = 540;

/// De quanto em quanto tempo a miniatura é atualizada.
pub const PREVIEW_INTERVAL: Duration = Duration::from_millis(100);

/// Miniatura em BGRA (4 bytes por pixel), o formato nativo do Avalonia no Windows.
struct PreviewFrame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Caixa compartilhada entre a transmissão (que escreve) e a interface (que lê).
#[derive(Clone, Default)]
pub struct PreviewSlot {
    inner: Arc<Mutex<Option<PreviewFrame>>>,
}

impl PreviewSlot {
    /// Reduz o frame capturado e guarda como a miniatura mais recente.
    pub fn publish(&self, frame: &CapturedFrame) {
        if let Some(thumb) = thumbnail(frame) {
            *self.lock() = Some(thumb);
        }
    }

    /// Apaga a miniatura (a transmissão terminou).
    pub fn clear(&self) {
        *self.lock() = None;
    }

    /// Copia a miniatura para `out`. Devolve (largura, altura) ou None se não há
    /// miniatura ou se `out` é pequeno demais.
    pub fn read_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        let guard = self.lock();
        let frame = guard.as_ref()?;
        if out.len() < frame.data.len() {
            return None;
        }
        out[..frame.data.len()].copy_from_slice(&frame.data);
        Some((frame.width, frame.height))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<PreviewFrame>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Reduz RGBA -> BGRA. Cada pixel novo é a média de 4 pixels do bloco original (suaviza sem pesar).
fn thumbnail(frame: &CapturedFrame) -> Option<PreviewFrame> {
    let (src_w, src_h) = (frame.width as usize, frame.height as usize);
    if src_w == 0 || src_h == 0 || frame.data.len() < src_w * src_h * 4 {
        return None;
    }

    // Nunca amplia; só reduz até caber na caixa
    let scale = (PREVIEW_MAX_WIDTH as f64 / src_w as f64)
        .min(PREVIEW_MAX_HEIGHT as f64 / src_h as f64)
        .min(1.0);
    let dst_w = ((src_w as f64 * scale) as usize).max(2);
    let dst_h = ((src_h as f64 * scale) as usize).max(2);

    let mut data = vec![0u8; dst_w * dst_h * 4];
    for dy in 0..dst_h {
        let sy0 = (dy * src_h / dst_h).min(src_h - 1);
        let sy1 = (((dy + 1) * src_h / dst_h).saturating_sub(1)).clamp(sy0, src_h - 1);
        for dx in 0..dst_w {
            let sx0 = (dx * src_w / dst_w).min(src_w - 1);
            let sx1 = (((dx + 1) * src_w / dst_w).saturating_sub(1)).clamp(sx0, src_w - 1);

            let corners = [(sx0, sy0), (sx1, sy0), (sx0, sy1), (sx1, sy1)];
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for (x, y) in corners {
                let s = (y * src_w + x) * 4;
                r += frame.data[s] as u32;
                g += frame.data[s + 1] as u32;
                b += frame.data[s + 2] as u32;
            }

            let o = (dy * dst_w + dx) * 4;
            data[o] = (b / 4) as u8; // B
            data[o + 1] = (g / 4) as u8; // G
            data[o + 2] = (r / 4) as u8; // R
            data[o + 3] = 255;
        }
    }

    Some(PreviewFrame {
        width: dst_w as u32,
        height: dst_h as u32,
        data,
    })
}
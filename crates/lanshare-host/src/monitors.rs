//! Monitores do Host: lista os monitores e tira uma foto de cada um
//! (usado no painel de escolha do que transmitir).

use crate::capture::ScreenCapture;
use rayon::prelude::*;
use serde::Serialize;
use std::error::Error;
use std::time::Duration;
use windows_capture::monitor::Monitor;

type BoxError = Box<dyn Error + Send + Sync>;

/// Tamanho máximo da miniatura. A interface (C#) reserva um buffer deste tamanho: mantenha igual.
pub const THUMB_MAX_WIDTH: usize = 400;
pub const THUMB_MAX_HEIGHT: usize = 240;

/// Tempo máximo esperando o monitor entregar a primeira imagem.
const THUMB_TIMEOUT: Duration = Duration::from_millis(2500);

/// Um monitor, como a interface o enxerga.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// Posição na lista do Windows (é o número que a transmissão usa para achar o monitor).
    pub index: usize,
    /// Ex.: "Monitor 1 - DELL U2419H".
    pub name: String,
    /// Modelo informado pelo Windows (ou "Genérico").
    pub model: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// Foto reduzida de um monitor (pixels BGRA, 4 bytes por pixel).
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    /// Tamanho real do monitor.
    pub source_width: u32,
    pub source_height: u32,
    pub bgra: Vec<u8>,
}

/// Lista os monitores, com o principal primeiro.
pub fn list_monitors() -> Result<Vec<MonitorInfo>, BoxError> {
    let monitors = Monitor::enumerate()?;
    let primary = Monitor::primary().ok().map(|m| m.as_raw_hmonitor());

    let mut order: Vec<(usize, bool)> = monitors
        .iter()
        .enumerate()
        .map(|(index, m)| (index, Some(m.as_raw_hmonitor()) == primary))
        .collect();

    // Se o Windows não disse qual é o principal, o primeiro da lista faz esse papel
    if !order.is_empty() && !order.iter().any(|&(_, is_primary)| is_primary) {
        order[0].1 = true;
    }

    // Principal primeiro (a ordenação é estável: os outros mantêm a ordem do Windows)
    order.sort_by_key(|&(_, is_primary)| !is_primary);

    Ok(order
        .into_iter()
        .enumerate()
        .map(|(position, (index, is_primary))| {
            let monitor = &monitors[index];
            let model = model_name(monitor);
            MonitorInfo {
                index,
                name: format!("Monitor {} - {model}", position + 1),
                model,
                width: monitor.width().unwrap_or(0),
                height: monitor.height().unwrap_or(0),
                primary: is_primary,
            }
        })
        .collect())
}

/// Nome do modelo que o Windows leu do próprio monitor (ex.: "DELL U2419H").
fn model_name(monitor: &Monitor) -> String {
    monitor
        .name()
        .ok()
        .map(|name| name.trim().to_string())
        // Sem o nome do modelo, alguns sistemas devolvem só o nome técnico (\\.\DISPLAY1)
        .filter(|name| !name.is_empty() && !name.starts_with(r"\\.\"))
        .unwrap_or_else(|| "Genérico".to_string())
}

/// Captura uma imagem do monitor e a reduz para a miniatura.
/// Leva uma fração de segundo; não use na thread da interface.
pub fn monitor_thumbnail(index: usize) -> Result<Thumbnail, BoxError> {
    let capture = ScreenCapture::start_quiet(index)?;
    let received = capture.frames.recv_timeout(THUMB_TIMEOUT);
    let _ = capture.stop();
    let frame = received.map_err(|_| "o monitor nao entregou nenhuma imagem")?;

    let (src_w, src_h) = (frame.width as usize, frame.height as usize);
    if src_w == 0 || src_h == 0 || frame.data.len() < src_w * src_h * 4 {
        return Err("imagem do monitor invalida".into());
    }

    // Nunca amplia; mantém a proporção do monitor
    let scale = (THUMB_MAX_WIDTH as f64 / src_w as f64)
        .min(THUMB_MAX_HEIGHT as f64 / src_h as f64)
        .min(1.0);
    let dst_w = ((src_w as f64 * scale) as usize).clamp(2, THUMB_MAX_WIDTH);
    let dst_h = ((src_h as f64 * scale) as usize).clamp(2, THUMB_MAX_HEIGHT);

    let data = &frame.data;
    let mut bgra = vec![0u8; dst_w * dst_h * 4];

    // Cada pixel novo é a média dos pixels originais que ele cobre; uma linha por tarefa
    bgra.par_chunks_mut(dst_w * 4)
        .enumerate()
        .for_each(|(dy, row)| {
            let sy0 = dy * src_h / dst_h;
            let sy1 = ((dy + 1) * src_h / dst_h).max(sy0 + 1).min(src_h);

            for dx in 0..dst_w {
                let sx0 = dx * src_w / dst_w;
                let sx1 = ((dx + 1) * src_w / dst_w).max(sx0 + 1).min(src_w);

                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for sy in sy0..sy1 {
                    for px in data[(sy * src_w + sx0) * 4..(sy * src_w + sx1) * 4].chunks_exact(4) {
                        r += px[0] as u32;
                        g += px[1] as u32;
                        b += px[2] as u32;
                    }
                }

                let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
                let o = dx * 4;
                row[o] = (b / n) as u8;
                row[o + 1] = (g / n) as u8;
                row[o + 2] = (r / n) as u8;
                row[o + 3] = 255;
            }
        });

    Ok(Thumbnail {
        width: dst_w as u32,
        height: dst_h as u32,
        source_width: frame.width as u32,
        source_height: frame.height as u32,
        bgra,
    })
}
// Compressão dos frames capturados em H.264 (software, via OpenH264).

use std::error::Error;
use std::time::{Duration, Instant};

use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode, UsageType};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use openh264::OpenH264API;

use crate::capture::{CapturedFrame, ScreenCapture};

type BoxError = Box<dyn Error + Send + Sync>;

/// Valores provisórios; depois viram opções (resolução/FPS selecionáveis).
const TARGET_FPS: f32 = 30.0;
const TARGET_BITRATE_BPS: u32 = 6_000_000; // 6 Mbps

/// Encoder H.264. Guarda o estado entre frames (é assim que ele
/// consegue mandar só o que mudou).
pub struct VideoEncoder {
    encoder: Encoder,
}

impl VideoEncoder {
    pub fn new(fps: f32, bitrate_bps: u32) -> Result<Self, BoxError> {
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime) // otimizado para tela
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(fps));

        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)?;
        Ok(Self { encoder })
    }

    /// Comprime um frame RGBA em H.264.
    /// Devolve None se o encoder decidiu não gerar dados para este frame.
    pub fn encode(&mut self, frame: &CapturedFrame) -> Result<Option<Vec<u8>>, BoxError> {
        let rgba = RgbaSliceU8::new(&frame.data, (frame.width as usize, frame.height as usize));
        let yuv = YUVBuffer::from_rgb_source(rgba);

        let bitstream = self.encoder.encode(&yuv)?;
        let data = bitstream.to_vec();

        if data.is_empty() {
            Ok(None)
        } else {
            Ok(Some(data))
        }
    }
}

/// Teste temporário: captura, comprime e mostra as estatísticas no terminal.
pub fn test_encode(seconds: u64) -> Result<(), BoxError> {
    let capture = ScreenCapture::start()?;
    let mut encoder = VideoEncoder::new(TARGET_FPS, TARGET_BITRATE_BPS)?;

    let start = Instant::now();
    let mut frames_in: u32 = 0;
    let mut frames_out: u32 = 0;
    let mut raw_bytes: u64 = 0;
    let mut encoded_bytes: u64 = 0;
    let mut encode_time = Duration::ZERO;
    let mut size = (0, 0);

    while start.elapsed() < Duration::from_secs(seconds) {
        let Ok(frame) = capture.frames.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };

        frames_in += 1;
        size = (frame.width, frame.height);
        raw_bytes += frame.data.len() as u64;

        let t = Instant::now();
        let packet = encoder.encode(&frame)?;
        encode_time += t.elapsed();

        if let Some(packet) = packet {
            frames_out += 1;
            encoded_bytes += packet.len() as u64;
        }
    }

    capture.stop()?;

    if frames_in == 0 || frames_out == 0 {
        println!("Nenhum frame foi codificado. Mexa a tela durante o teste.");
        return Ok(());
    }

    let avg_encode_ms = encode_time.as_secs_f64() * 1000.0 / frames_in as f64;
    let avg_raw_kb = raw_bytes as f64 / frames_in as f64 / 1024.0;
    let avg_encoded_kb = encoded_bytes as f64 / frames_out as f64 / 1024.0;
    let mbps = encoded_bytes as f64 * 8.0 / seconds as f64 / 1_000_000.0;

    println!("Resolução: {}x{}", size.0, size.1);
    println!("Frames capturados: {frames_in} | frames codificados: {frames_out}");
    println!("Tamanho médio por frame: {avg_raw_kb:.0} KB -> {avg_encoded_kb:.1} KB");
    println!("Compressão: {:.0}x", avg_raw_kb / avg_encoded_kb);
    println!("Bitrate médio: {mbps:.2} Mbps");
    println!("Tempo médio para codificar 1 frame: {avg_encode_ms:.1} ms");

    // Para 30 FPS, cada frame precisa ser codificado em menos de ~33 ms
    if avg_encode_ms > 33.0 {
        println!("Aviso: acima de 33 ms, esta CPU não sustenta 30 FPS neste encoder.");
    } else {
        println!("OK: o tempo de codificação cabe em 30 FPS.");
    }

    Ok(())
}
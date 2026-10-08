// Compressão H.264 por GPU, via Media Foundation (encoder de hardware do Windows).

use std::error::Error;
use std::mem::ManuallyDrop;
use std::ptr;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use windows::core::{Interface, GUID};
use windows::Win32::Foundation::{VARIANT_BOOL, VARIANT_TRUE};
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, eAVEncCommonRateControlMode_PeakConstrainedVBR,
    eAVEncH264VProfile_High, CODECAPI_AVEncCommonMaxBitRate, CODECAPI_AVEncCommonMeanBitRate,
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize,
    CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFActivate,
    IMFMediaEvent, IMFMediaEventGenerator, IMFSample, IMFTransform, METransformHaveOutput,
    METransformNeedInput, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, MFNominalRange_16_235, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
    MF_E_NO_EVENTS_AVAILABLE, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_EVENT_FLAG_NONE, MF_EVENT_FLAG_NO_WAIT, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE,
    MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_MT_YUV_MATRIX, MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFSTARTUP_FULL,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_UI4};

use crate::capture::{CapturedFrame, ScreenCapture};

type BoxError = Box<dyn Error + Send + Sync>;

/// Valores do teste `hwencode` (1080p a 30 FPS); na transmissão valem as opções do `share`.
pub const TARGET_FPS: u32 = 30;
pub const TARGET_BITRATE_BPS: u32 = 6_000_000; // 6 Mbps (média alvo)

/// Média alvo de bitrate para cada combinação de resolução e FPS escolhida no `share`.
pub fn bitrate_for(height: u32, fps: u32) -> u32 {
    match (height, fps) {
        (720, 15) => 2_000_000,
        (720, _) => 3_000_000,
        (_, 15) => 4_500_000,
        _ => TARGET_BITRATE_BPS,
    }
}

/// Unidade de tempo do Media Foundation: 100 nanossegundos.
const HNS_PER_SEC: i64 = 10_000_000;

fn mf(what: &str, erro: windows::core::Error) -> BoxError {
    format!("{what}: {erro}").into()
}

/// Dois números de 32 bits num só de 64 (formato que o Media Foundation usa).
fn pack_2x32(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

/// Tamanho da imagem transmitida: a altura pedida, com a largura na mesma proporção da tela.
/// Os dois valores saem pares (o encoder exige). Nunca amplia: se a tela já é menor
/// que a altura pedida, mantém o tamanho dela.
fn scaled_size(src_width: u32, src_height: u32, target_height: u32) -> (u32, u32) {
    if src_height <= target_height {
        return (src_width & !1, src_height & !1);
    }

    let height = target_height & !1;
    let width = ((src_width as u64 * height as u64 / src_height as u64) as u32) & !1;
    (width, height)
}

/// Liga o COM e o Media Foundation enquanto existir; desliga ao sair.
struct MfRuntime;

impl MfRuntime {
    fn new() -> Result<Self, BoxError> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| mf("CoInitializeEx", e))?;
            if let Err(erro) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(mf("MFStartup", erro));
            }
        }
        Ok(Self)
    }
}

impl Drop for MfRuntime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

/// Encoder H.264 de hardware. Deve ser criado e usado na MESMA thread.
pub struct HwEncoder {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    codec_api: ICodecAPI,
    /// Tamanho da tela capturada (os frames que chegam têm sempre este tamanho).
    src_width: u32,
    src_height: u32,
    /// Tamanho da imagem transmitida (já reduzida, se for o caso).
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    needs_input: bool,
    provides_samples: bool,
    output_size: u32,
    sample_index: i64,
    /// Buffer reaproveitado para a imagem reduzida (RGBA).
    scratch: Vec<u8>,
    /// Quanto tempo a última redução + conversão RGBA -> NV12 levou (para o teste).
    pub last_convert: Duration,
    _runtime: MfRuntime, // por último: precisa ser liberado depois do encoder
}

impl HwEncoder {
    /// `src_width` x `src_height` é o tamanho da tela capturada; `target_height` é a altura
    /// pedida para a transmissão (a imagem é reduzida se a tela for maior).
    pub fn new(
        src_width: u32,
        src_height: u32,
        target_height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<Self, BoxError> {
        let (width, height) = scaled_size(src_width, src_height, target_height);
        if width < 2 || height < 2 {
            return Err(format!("resolucao {src_width}x{src_height} nao suportada").into());
        }

        let runtime = MfRuntime::new()?;
        let transform = find_hardware_encoder()?;

        // Encoders de hardware são assíncronos: precisa destravar antes de usar
        let attributes = unsafe { transform.GetAttributes() }.map_err(|e| mf("GetAttributes", e))?;
        unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
            .map_err(|e| mf("async unlock", e))?;

        let events = transform
            .cast::<IMFMediaEventGenerator>()
            .map_err(|e| mf("eventos do encoder", e))?;
        let codec_api = transform
            .cast::<ICodecAPI>()
            .map_err(|e| mf("ICodecAPI", e))?;

        let mut encoder = Self {
            transform,
            events,
            codec_api,
            src_width,
            src_height,
            width,
            height,
            fps,
            bitrate,
            needs_input: false,
            provides_samples: false,
            output_size: 0,
            sample_index: 0,
            scratch: Vec::new(),
            last_convert: Duration::ZERO,
            _runtime: runtime,
        };

        encoder.configure_codec();
        encoder.set_output_type()?;
        encoder.set_input_type()?;
        encoder.read_output_info()?;

        unsafe {
            encoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| mf("begin streaming", e))?;
            encoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| mf("start of stream", e))?;
        }

        Ok(encoder)
    }

    /// Tamanho (largura, altura) da imagem que sai do encoder.
    pub fn frame_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Comprime um frame RGBA. Devolve 0 ou mais pacotes H.264
    /// (o encoder de hardware pode entregar a saída um pouco depois da entrada).
    pub fn encode(&mut self, frame: &CapturedFrame) -> Result<Vec<Vec<u8>>, BoxError> {
        if frame.width != self.src_width || frame.height != self.src_height {
            return Err("a resolucao mudou no meio da captura (ainda nao suportado)".into());
        }

        let mut packets = Vec::new();

        // Espera o encoder pedir mais um frame
        while !self.needs_input {
            let event = unsafe { self.events.GetEvent(MF_EVENT_FLAG_NONE) }
                .map_err(|e| mf("GetEvent", e))?;
            self.handle_event(&event, &mut packets)?;
        }

        let sample = self.build_sample(frame)?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|e| mf("ProcessInput", e))?;
        self.needs_input = false;
        self.sample_index += 1;

        // Recolhe o que já estiver pronto, sem esperar
        loop {
            match unsafe { self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => self.handle_event(&event, &mut packets)?,
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                Err(e) => return Err(mf("GetEvent", e)),
            }
        }

        Ok(packets)
    }

    /// Faz o próximo frame codificado sair como keyframe (quadro completo).
    pub fn force_keyframe(&self) {
        self.set_codec(&CODECAPI_AVEncVideoForceKeyFrame, variant_u32(1));
    }

    /// Teto do bitrate no modo VBR: 1,5x a média alvo.
    fn max_bitrate(&self) -> u32 {
        self.bitrate / 2 * 3
    }

    fn configure_codec(&self) {
        // Baixa latência: sem B-frames nem fila interna
        self.set_codec(&CODECAPI_AVLowLatencyMode, variant_bool(true));

        // VBR com teto: gasta menos em cena simples e tem fôlego em movimento.
        // Se o driver recusar, volta para CBR (taxa constante).
        let vbr_ok = self.set_codec(
            &CODECAPI_AVEncCommonRateControlMode,
            variant_u32(eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32),
        );
        if vbr_ok {
            self.set_codec(&CODECAPI_AVEncCommonMaxBitRate, variant_u32(self.max_bitrate()));
            println!(
                "[hw] Controle de taxa: VBR (media {:.1} Mbps, teto {:.1} Mbps)",
                self.bitrate as f64 / 1_000_000.0,
                self.max_bitrate() as f64 / 1_000_000.0
            );
        } else {
            self.set_codec(
                &CODECAPI_AVEncCommonRateControlMode,
                variant_u32(eAVEncCommonRateControlMode_CBR.0 as u32),
            );
            println!(
                "[hw] Controle de taxa: CBR de {:.1} Mbps (o encoder recusou o VBR)",
                self.bitrate as f64 / 1_000_000.0
            );
        }

        self.set_codec(&CODECAPI_AVEncCommonMeanBitRate, variant_u32(self.bitrate));
        // Um keyframe a cada 2 segundos
        self.set_codec(&CODECAPI_AVEncMPVGOPSize, variant_u32(self.fps * 2));
    }

    // Esses ajustes são sugestões: se o driver recusar, seguimos mesmo assim.
    // Devolve true se o encoder aceitou o ajuste.
    fn set_codec(&self, api: *const GUID, value: VARIANT) -> bool {
        match unsafe { self.codec_api.SetValue(api, &value) } {
            Ok(()) => true,
            Err(erro) => {
                println!("[hw] Aviso: o encoder recusou um ajuste ({erro})");
                false
            }
        }
    }

    fn set_color(&self, media: &windows::Win32::Media::MediaFoundation::IMFMediaType) -> Result<(), BoxError> {
        // BT.709, faixa limitada (16-235): é o padrão para vídeo HD
        unsafe {
            media
                .SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)
                .map_err(|e| mf("primarias", e))?;
            media
                .SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
                .map_err(|e| mf("transferencia", e))?;
            media
                .SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
                .map_err(|e| mf("matriz", e))?;
            media
                .SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
                .map_err(|e| mf("faixa", e))?;
        }
        Ok(())
    }

    fn set_output_type(&self) -> Result<(), BoxError> {
        let media = unsafe { MFCreateMediaType() }.map_err(|e| mf("MFCreateMediaType", e))?;
        unsafe {
            media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| mf("saida: tipo", e))?;
            media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).map_err(|e| mf("saida: formato", e))?;
            media.SetUINT32(&MF_MT_AVG_BITRATE, self.bitrate).map_err(|e| mf("saida: bitrate", e))?;
            media
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(|e| mf("saida: interlace", e))?;
            media
                .SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)
                .map_err(|e| mf("saida: perfil", e))?;
            media
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_2x32(self.width, self.height))
                .map_err(|e| mf("saida: tamanho", e))?;
            media
                .SetUINT64(&MF_MT_FRAME_RATE, pack_2x32(self.fps, 1))
                .map_err(|e| mf("saida: fps", e))?;
        }
        self.set_color(&media)?;
        unsafe { self.transform.SetOutputType(0, &media, 0) }.map_err(|e| mf("SetOutputType", e))?;
        Ok(())
    }

    fn set_input_type(&self) -> Result<(), BoxError> {
        let media = unsafe { MFCreateMediaType() }.map_err(|e| mf("MFCreateMediaType", e))?;
        unsafe {
            media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| mf("entrada: tipo", e))?;
            media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).map_err(|e| mf("entrada: formato", e))?;
            media
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(|e| mf("entrada: interlace", e))?;
            media
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_2x32(self.width, self.height))
                .map_err(|e| mf("entrada: tamanho", e))?;
            media
                .SetUINT64(&MF_MT_FRAME_RATE, pack_2x32(self.fps, 1))
                .map_err(|e| mf("entrada: fps", e))?;
        }
        self.set_color(&media)?;
        unsafe { self.transform.SetInputType(0, &media, 0) }.map_err(|e| mf("SetInputType", e))?;
        Ok(())
    }

    /// Descobre se o encoder aloca a própria saída e, se não, o tamanho do buffer.
    fn read_output_info(&mut self) -> Result<(), BoxError> {
        let info = unsafe { self.transform.GetOutputStreamInfo(0) }
            .map_err(|e| mf("GetOutputStreamInfo", e))?;
        self.provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        self.output_size = info.cbSize;
        Ok(())
    }

    /// Reduz o frame (se preciso) e converte para NV12 direto dentro do buffer do Media Foundation.
    fn build_sample(&mut self, frame: &CapturedFrame) -> Result<IMFSample, BoxError> {
        let (w, h) = (self.width as usize, self.height as usize);
        let len = w * h * 3 / 2;

        let buffer = unsafe { MFCreateMemoryBuffer(len as u32) }
            .map_err(|e| mf("MFCreateMemoryBuffer", e))?;

        let mut data: *mut u8 = ptr::null_mut();
        unsafe { buffer.Lock(&mut data, None, None) }.map_err(|e| mf("Lock", e))?;

        let started = Instant::now();

        // Se a resolução pedida é menor que a da tela, reduz a imagem antes de converter
        let rgba: &[u8] = if frame.width != self.width || frame.height != self.height {
            scale_rgba(
                &frame.data,
                frame.width as usize,
                frame.height as usize,
                w,
                h,
                &mut self.scratch,
            );
            &self.scratch
        } else {
            &frame.data
        };

        // SAFETY: o buffer tem `len` bytes e fica travado até o Unlock abaixo
        let nv12 = unsafe { std::slice::from_raw_parts_mut(data, len) };
        rgba_to_nv12(rgba, w, h, nv12);
        self.last_convert = started.elapsed();

        unsafe {
            let _ = buffer.Unlock();
            buffer.SetCurrentLength(len as u32).map_err(|e| mf("SetCurrentLength", e))?;
        }

        let tick = HNS_PER_SEC / self.fps as i64;
        let sample = unsafe { MFCreateSample() }.map_err(|e| mf("MFCreateSample", e))?;
        unsafe {
            sample.AddBuffer(&buffer).map_err(|e| mf("AddBuffer", e))?;
            sample
                .SetSampleTime(self.sample_index * tick)
                .map_err(|e| mf("SetSampleTime", e))?;
            sample.SetSampleDuration(tick).map_err(|e| mf("SetSampleDuration", e))?;
        }
        Ok(sample)
    }

    fn handle_event(&mut self, event: &IMFMediaEvent, packets: &mut Vec<Vec<u8>>) -> Result<(), BoxError> {
        // Se o encoder avisar de um erro, falha aqui em vez de esperar para sempre
        unsafe { event.GetStatus() }
            .map_err(|e| mf("GetStatus", e))?
            .ok()
            .map_err(|e| mf("o encoder reportou um erro", e))?;

        const NEED_INPUT: u32 = METransformNeedInput.0 as u32;
        const HAVE_OUTPUT: u32 = METransformHaveOutput.0 as u32;

        match unsafe { event.GetType() }.map_err(|e| mf("GetType", e))? {
            NEED_INPUT => self.needs_input = true,
            HAVE_OUTPUT => {
                if let Some(packet) = self.process_output()? {
                    packets.push(packet);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Pega um pacote H.264 pronto do encoder.
    fn process_output(&mut self) -> Result<Option<Vec<u8>>, BoxError> {
        let provided = if self.provides_samples {
            None
        } else {
            let buffer = unsafe { MFCreateMemoryBuffer(self.output_size) }
                .map_err(|e| mf("buffer de saida", e))?;
            let sample = unsafe { MFCreateSample() }.map_err(|e| mf("sample de saida", e))?;
            unsafe { sample.AddBuffer(&buffer) }.map_err(|e| mf("AddBuffer", e))?;
            Some(sample)
        };

        let mut data = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(provided),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        }];
        let mut status = 0u32;
        let result = unsafe { self.transform.ProcessOutput(0, &mut data, &mut status) };

        // Assume a posse do que o encoder devolveu, para liberar certo
        let sample = ManuallyDrop::into_inner(unsafe { ptr::read(&data[0].pSample) });
        let _events = ManuallyDrop::into_inner(unsafe { ptr::read(&data[0].pEvents) });

        match result {
            Ok(()) => {}
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                // O encoder ajustou o formato de saída: reaplica e segue
                self.set_output_type()?;
                self.read_output_info()?;
                return Ok(None);
            }
            Err(e) => return Err(mf("ProcessOutput", e)),
        }

        let Some(sample) = sample else {
            return Ok(None);
        };
        Ok(Some(sample_to_bytes(&sample)?))
    }
}

impl Drop for HwEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

/// Procura o primeiro encoder H.264 de hardware (entrada NV12, saída H.264).
fn find_hardware_encoder() -> Result<IMFTransform, BoxError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };

    let mut activates: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count: u32 = 0;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut activates,
            &mut count,
        )
        .map_err(|e| mf("MFTEnumEx", e))?;
    }

    if count == 0 {
        return Err("nenhum encoder H.264 de hardware encontrado neste PC".into());
    }

    let entries = unsafe { std::slice::from_raw_parts_mut(activates, count as usize) };
    let mut found: Option<IMFTransform> = None;
    for slot in entries.iter_mut() {
        let Some(activate) = slot.take() else { continue };
        if found.is_none() {
            if let Ok(transform) = unsafe { activate.ActivateObject::<IMFTransform>() } {
                found = Some(transform);
            }
        }
    }
    unsafe { CoTaskMemFree(Some(activates as *const std::ffi::c_void)) };

    found.ok_or_else(|| "nao foi possivel ativar o encoder de hardware".into())
}

/// Copia os bytes de um sample de saída para um Vec.
fn sample_to_bytes(sample: &IMFSample) -> Result<Vec<u8>, BoxError> {
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|e| mf("ConvertToContiguousBuffer", e))?;

    let mut data: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    unsafe { buffer.Lock(&mut data, None, Some(&mut len)) }.map_err(|e| mf("Lock saida", e))?;
    let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec();
    unsafe {
        let _ = buffer.Unlock();
    }
    Ok(bytes)
}

/// Reduz uma imagem RGBA (4 bytes por pixel) para dst_w x dst_h.
/// Cada pixel novo é a média dos pixels originais que ele cobre; usa todos os núcleos da CPU.
fn scale_rgba(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    out: &mut Vec<u8>,
) {
    out.resize(dst_w * dst_h * 4, 0);

    // Cada tarefa cuida de uma linha da imagem reduzida
    out.par_chunks_mut(dst_w * 4)
        .enumerate()
        .for_each(|(dy, row)| {
            let sy0 = dy * src_h / dst_h;
            let sy1 = ((dy + 1) * src_h / dst_h).max(sy0 + 1).min(src_h);

            for dx in 0..dst_w {
                let sx0 = dx * src_w / dst_w;
                let sx1 = ((dx + 1) * src_w / dst_w).max(sx0 + 1).min(src_w);

                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for sy in sy0..sy1 {
                    for px in src[(sy * src_w + sx0) * 4..(sy * src_w + sx1) * 4].chunks_exact(4) {
                        r += px[0] as u32;
                        g += px[1] as u32;
                        b += px[2] as u32;
                    }
                }

                let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
                let o = dx * 4;
                row[o] = (r / n) as u8;
                row[o + 1] = (g / n) as u8;
                row[o + 2] = (b / n) as u8;
                row[o + 3] = 255;
            }
        });
}

/// RGBA -> NV12 (BT.709, faixa limitada), usando todos os núcleos da CPU.
/// NV12 = plano Y (brilho) seguido de um plano UV (cor) com metade da resolução.
fn rgba_to_nv12(rgba: &[u8], width: usize, height: usize, out: &mut [u8]) {
    let (y_plane, uv_plane) = out.split_at_mut(width * height);

    // Cada tarefa cuida de 2 linhas da imagem (que geram 1 linha de cor)
    y_plane
        .par_chunks_mut(width * 2)
        .zip(uv_plane.par_chunks_mut(width))
        .enumerate()
        .for_each(|(pair, (y_rows, uv_row))| {
            let top = &rgba[(pair * 2) * width * 4..][..width * 4];
            let bottom = &rgba[(pair * 2 + 1) * width * 4..][..width * 4];

            for col in (0..width).step_by(2) {
                let (mut r_sum, mut g_sum, mut b_sum) = (0i32, 0i32, 0i32);

                for (row, source) in [top, bottom].iter().enumerate() {
                    for dx in 0..2 {
                        let p = (col + dx) * 4;
                        let (r, g, b) = (source[p] as i32, source[p + 1] as i32, source[p + 2] as i32);

                        y_rows[row * width + col + dx] =
                            (((47 * r + 157 * g + 16 * b + 128) >> 8) + 16) as u8;

                        r_sum += r;
                        g_sum += g;
                        b_sum += b;
                    }
                }

                // A cor usa a média do bloco 2x2
                let (r, g, b) = (r_sum >> 2, g_sum >> 2, b_sum >> 2);
                uv_row[col] = (((-26 * r - 86 * g + 112 * b + 128) >> 8) + 128).clamp(16, 240) as u8;
                uv_row[col + 1] = (((112 * r - 102 * g - 10 * b + 128) >> 8) + 128).clamp(16, 240) as u8;
            }
        });
}

fn variant_u32(value: u32) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: preenche o campo da união que corresponde ao tipo marcado
    unsafe {
        let inner = &mut variant.Anonymous.Anonymous;
        inner.vt = VT_UI4;
        inner.Anonymous.ulVal = value;
    }
    variant
}

fn variant_bool(value: bool) -> VARIANT {
    let mut variant = VARIANT::default();
    unsafe {
        let inner = &mut variant.Anonymous.Anonymous;
        inner.vt = VT_BOOL;
        inner.Anonymous.boolVal = if value { VARIANT_TRUE } else { VARIANT_BOOL(0) };
    }
    variant
}

/// Teste temporário: captura, comprime na GPU e mostra as estatísticas.
pub fn test_hw_encode(seconds: u64) -> Result<(), BoxError> {
    let capture = ScreenCapture::start()?;
    let mut encoder: Option<HwEncoder> = None;

    let start = Instant::now();
    let mut frames_in: u32 = 0;
    let mut packets_out: u32 = 0;
    let mut encoded_bytes: u64 = 0;
    let mut convert_time = Duration::ZERO;
    let mut total_time = Duration::ZERO;
    let mut size = (0, 0);

    while start.elapsed() < Duration::from_secs(seconds) {
        let Ok(frame) = capture.frames.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };

        // O encoder nasce no primeiro frame, quando já sabemos a resolução
        if encoder.is_none() {
            size = (frame.width, frame.height);
            // Altura pedida = altura da tela: o teste roda na resolução nativa, sem redução
            encoder = Some(HwEncoder::new(
                frame.width,
                frame.height,
                frame.height,
                TARGET_FPS,
                TARGET_BITRATE_BPS,
            )?);
            println!("[hw] Encoder de hardware iniciado ({}x{}).", frame.width, frame.height);
        }
        let Some(encoder) = encoder.as_mut() else { continue };

        let t = Instant::now();
        let packets = encoder.encode(&frame)?;
        total_time += t.elapsed();
        convert_time += encoder.last_convert;

        frames_in += 1;
        for packet in packets {
            packets_out += 1;
            encoded_bytes += packet.len() as u64;
        }
    }

    capture.stop()?;

    if frames_in == 0 || packets_out == 0 {
        println!("Nenhum frame foi codificado. Mexa a tela durante o teste.");
        return Ok(());
    }

    let avg_total_ms = total_time.as_secs_f64() * 1000.0 / frames_in as f64;
    let avg_convert_ms = convert_time.as_secs_f64() * 1000.0 / frames_in as f64;
    let avg_encode_ms = (avg_total_ms - avg_convert_ms).max(0.0);
    let avg_packet_kb = encoded_bytes as f64 / packets_out as f64 / 1024.0;
    let mbps = encoded_bytes as f64 * 8.0 / seconds as f64 / 1_000_000.0;

    println!("Resolução: {}x{}", size.0, size.1);
    println!("Frames processados: {frames_in} em {seconds}s (~{:.1} FPS)", frames_in as f64 / seconds as f64);
    println!("Pacotes H.264 gerados: {packets_out} (média de {avg_packet_kb:.1} KB)");
    println!("Bitrate médio: {mbps:.2} Mbps");
    println!("Tempo médio por frame: {avg_total_ms:.1} ms");
    println!("  conversão RGBA -> NV12 (CPU): {avg_convert_ms:.1} ms");
    println!("  encoder + espera (GPU):       {avg_encode_ms:.1} ms");

    if avg_total_ms > 33.0 {
        println!("Aviso: acima de 33 ms, ainda nao sustenta 30 FPS.");
    } else {
        println!("OK: o tempo por frame cabe em 30 FPS.");
    }

    Ok(())
}
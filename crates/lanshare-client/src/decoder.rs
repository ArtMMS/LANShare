//! Decodificador H.264 do Client, via Media Foundation (decodificador do Windows).
//! Entra: pacotes H.264 (Annex B). Sai: imagens NV12, que viram pixels RGB.

use std::error::Error;
use std::mem::ManuallyDrop;
use std::ptr;

use rayon::prelude::*;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MSH264DecoderMFT, IMFSample, IMFTransform, MFCreateAlignedMemoryBuffer,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFShutdown,
    MFStartup, MFVideoFormat_H264, MFVideoFormat_NV12, MFSTARTUP_FULL, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_E_TRANSFORM_TYPE_NOT_SET, MF_LOW_LATENCY, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

type BoxError = Box<dyn Error + Send + Sync>;

/// Tamanho do buffer de saída antes de sabermos a resolução (só um valor válido qualquer).
const INITIAL_OUTPUT_SIZE: u32 = 4 * 1024 * 1024;

fn mf(what: &str, erro: windows::core::Error) -> BoxError {
    format!("{what}: {erro}").into()
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

/// Como o decodificador está entregando as imagens.
#[derive(Clone, Copy)]
struct OutputFormat {
    stride: usize,
    /// Tamanho visível da imagem (já sem as linhas extras de alinhamento).
    width: usize,
    height: usize,
}

/// Uma imagem decodificada, ainda em NV12 (plano Y + plano UV).
pub struct Picture {
    pub width: usize,
    pub height: usize,
    stride: usize,
    uv_offset: usize,
    data: Vec<u8>,
}

impl Picture {
    fn from_nv12(data: Vec<u8>, format: OutputFormat) -> Result<Self, BoxError> {
        // O tamanho real do buffer diz quantas linhas o plano Y tem (pode ter linhas extras)
        let rows = data.len() * 2 / (3 * format.stride);
        if rows < format.height || format.stride < format.width {
            return Err(format!(
                "imagem decodificada com tamanho inesperado ({} bytes para {}x{}, stride {})",
                data.len(),
                format.width,
                format.height,
                format.stride
            )
            .into());
        }

        Ok(Self {
            width: format.width,
            height: format.height,
            stride: format.stride,
            uv_offset: format.stride * rows,
            data,
        })
    }

    /// NV12 -> pixels 0x00RRGGBB (BT.709, faixa limitada, igual ao encoder do Host).
    pub fn to_rgb(&self, out: &mut Vec<u32>) {
        out.resize(self.width * self.height, 0);
        let (y_plane, uv_plane) = self.data.split_at(self.uv_offset);

        out.par_chunks_mut(self.width)
            .enumerate()
            .for_each(|(row, pixels)| {
                let y_row = &y_plane[row * self.stride..];
                let uv_row = &uv_plane[(row / 2) * self.stride..];

                for (col, pixel) in pixels.iter_mut().enumerate() {
                    let pair = col & !1;
                    let y = y_row[col] as i32 - 16;
                    let u = uv_row[pair] as i32 - 128;
                    let v = uv_row[pair + 1] as i32 - 128;

                    let base = 298 * y;
                    let r = ((base + 459 * v + 128) >> 8).clamp(0, 255) as u32;
                    let g = ((base - 55 * u - 136 * v + 128) >> 8).clamp(0, 255) as u32;
                    let b = ((base + 541 * u + 128) >> 8).clamp(0, 255) as u32;

                    *pixel = (r << 16) | (g << 8) | b;
                }
            });
    }
}

/// Decodificador H.264. Deve ser criado e usado na MESMA thread.
pub struct Decoder {
    transform: IMFTransform,
    format: Option<OutputFormat>,
    provides_samples: bool,
    /// Tamanho e alinhamento (em bytes) que o decodificador exige do buffer de saída.
    output_size: u32,
    output_alignment: u32,
    _runtime: MfRuntime, // por último: precisa ser liberado depois do decodificador
}

impl Decoder {
    pub fn new() -> Result<Self, BoxError> {
        let runtime = MfRuntime::new()?;

        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| mf("criar o decodificador H.264", e))?;

        // Baixa latência: entrega a imagem assim que ela é decodificada
        match unsafe { transform.GetAttributes() } {
            Ok(attributes) => {
                if let Err(erro) = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) } {
                    println!("[client] Aviso: nao foi possivel ligar a baixa latencia ({erro})");
                }
            }
            Err(erro) => println!("[client] Aviso: sem acesso aos atributos do decodificador ({erro})"),
        }

        // Entrada: H.264. A resolução o decodificador descobre sozinho pelo SPS do stream.
        let input = unsafe { MFCreateMediaType() }.map_err(|e| mf("MFCreateMediaType", e))?;
        unsafe {
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| mf("entrada: tipo", e))?;
            input
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(|e| mf("entrada: formato", e))?;
            transform
                .SetInputType(0, &input, 0)
                .map_err(|e| mf("SetInputType", e))?;

            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| mf("begin streaming", e))?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| mf("start of stream", e))?;
        }

        Ok(Self {
            transform,
            format: None,
            provides_samples: false,
            output_size: 0,
            output_alignment: 0,
            _runtime: runtime,
        })
    }

    /// Joga fora o que o decodificador guardou (usado antes de recomeçar num keyframe).
    pub fn flush(&self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }

    /// Decodifica um pacote H.264. Devolve 0 ou mais imagens.
    pub fn decode(&mut self, data: &[u8], timestamp_us: u64) -> Result<Vec<Picture>, BoxError> {
        let sample = build_input_sample(data, timestamp_us)?;
        let mut pictures = Vec::new();

        for _ in 0..8 {
            match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
                Ok(()) => {
                    // Aceitou: recolhe tudo o que já estiver pronto e termina
                    while let Some(picture) = self.pull_output()? {
                        pictures.push(picture);
                    }
                    return Ok(pictures);
                }
                // Está cheio: esvazia a saída e tenta de novo
                Err(e) if e.code() == MF_E_NOTACCEPTING => {
                    while let Some(picture) = self.pull_output()? {
                        pictures.push(picture);
                    }
                }
                Err(e) => return Err(mf("ProcessInput", e)),
            }
        }

        Err("o decodificador nao aceitou o pacote".into())
    }

    /// Pega uma imagem pronta do decodificador (None = precisa de mais entrada).
    fn pull_output(&mut self) -> Result<Option<Picture>, BoxError> {
        // Se o decodificador pedir o formato de saída, escolhemos e tentamos de novo
        for _ in 0..4 {
            // Buffer novo a cada tentativa (reaproveitar o anterior fazia o decodificador falhar)
            let provided = if self.provides_samples {
                None
            } else {
                Some(self.new_output_sample()?)
            };

            let mut data = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(provided),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            let result = unsafe { self.transform.ProcessOutput(0, &mut data, &mut status) };

            // Assume a posse do que o decodificador devolveu, para liberar certo
            let sample = ManuallyDrop::into_inner(unsafe { ptr::read(&data[0].pSample) });
            let _events = ManuallyDrop::into_inner(unsafe { ptr::read(&data[0].pEvents) });

            match result {
                Ok(()) => {}
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
                // Os dois avisos significam o mesmo: "escolha o formato de saída"
                Err(e)
                    if e.code() == MF_E_TRANSFORM_STREAM_CHANGE
                        || e.code() == MF_E_TRANSFORM_TYPE_NOT_SET =>
                {
                    if self.negotiate_output()? {
                        continue;
                    }
                    // Ainda não leu o SPS: espera o próximo pacote
                    return Ok(None);
                }
                Err(e) => return Err(mf("ProcessOutput", e)),
            }

            let Some(sample) = sample else {
                return Ok(None);
            };
            let Some(format) = self.format else {
                return Err("o decodificador entregou uma imagem sem formato definido".into());
            };

            let bytes = sample_to_bytes(&sample)?;
            return Ok(Some(Picture::from_nv12(bytes, format)?));
        }

        Err("o decodificador nao estabilizou o formato de saida".into())
    }

    /// Cria um buffer de saída novo, no tamanho e alinhamento que o decodificador pede.
    fn new_output_sample(&self) -> Result<IMFSample, BoxError> {
        let size = if self.output_size > 0 {
            self.output_size
        } else {
            INITIAL_OUTPUT_SIZE
        };

        // O Media Foundation pede o alinhamento como "bytes - 1" (0 = sem alinhamento)
        let alignment = self.output_alignment.saturating_sub(1);
        let buffer = unsafe { MFCreateAlignedMemoryBuffer(size, alignment) }
            .map_err(|e| mf("buffer de saida", e))?;

        let sample = unsafe { MFCreateSample() }.map_err(|e| mf("sample de saida", e))?;
        unsafe { sample.AddBuffer(&buffer) }.map_err(|e| mf("AddBuffer", e))?;
        Ok(sample)
    }

    /// Escolhe NV12 como saída e lê tamanho e stride da imagem.
    /// Devolve false se o decodificador ainda não sabe oferecer formatos (falta o SPS).
    fn negotiate_output(&mut self) -> Result<bool, BoxError> {
        let mut index = 0u32;
        loop {
            let media = match unsafe { self.transform.GetOutputAvailableType(0, index) } {
                Ok(media) => media,
                Err(e) if index == 0 && e.code() == MF_E_TRANSFORM_TYPE_NOT_SET => {
                    return Ok(false);
                }
                Err(_) => return Err("o decodificador nao oferece saida NV12".into()),
            };
            let subtype =
                unsafe { media.GetGUID(&MF_MT_SUBTYPE) }.map_err(|e| mf("saida: subtipo", e))?;

            if subtype == MFVideoFormat_NV12 {
                unsafe { self.transform.SetOutputType(0, &media, 0) }
                    .map_err(|e| mf("SetOutputType", e))?;
                break;
            }
            index += 1;
        }

        let current = unsafe { self.transform.GetOutputCurrentType(0) }
            .map_err(|e| mf("GetOutputCurrentType", e))?;

        let packed = unsafe { current.GetUINT64(&MF_MT_FRAME_SIZE) }
            .map_err(|e| mf("saida: tamanho", e))?;
        let coded_w = (packed >> 32) as usize;
        let coded_h = (packed & 0xFFFF_FFFF) as usize;

        let stride = unsafe { current.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
            .ok()
            .map(|s| s as i32)
            .filter(|s| *s > 0)
            .map(|s| s as usize)
            .unwrap_or(coded_w)
            .max(coded_w);

        // O decodificador pode usar linhas extras (ex.: 1088 para 1080); a área visível vem daqui
        let mut area = [0u8; 16];
        let (width, height) =
            if unsafe { current.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None) }.is_ok() {
                let cx = i32::from_le_bytes([area[8], area[9], area[10], area[11]]);
                let cy = i32::from_le_bytes([area[12], area[13], area[14], area[15]]);
                if cx > 0 && cy > 0 && cx as usize <= coded_w && cy as usize <= coded_h {
                    (cx as usize, cy as usize)
                } else {
                    (coded_w, coded_h)
                }
            } else {
                (coded_w, coded_h)
            };

        let info = unsafe { self.transform.GetOutputStreamInfo(0) }
            .map_err(|e| mf("GetOutputStreamInfo", e))?;
        self.provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;

        // Folga: nunca menor que a imagem NV12 completa (com as linhas extras)
        let full_frame = (stride * coded_h * 3 / 2) as u32;
        self.output_size = info.cbSize.max(full_frame);
        self.output_alignment = info.cbAlignment;

        self.format = Some(OutputFormat {
            stride,
            width,
            height,
        });

        println!(
            "[client] Decodificador pronto: {width}x{height} (codificado {coded_w}x{coded_h}, stride {stride}, buffer {} bytes, alinhamento {})",
            self.output_size, self.output_alignment
        );
        Ok(true)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

/// Coloca os bytes H.264 recebidos num sample do Media Foundation.
fn build_input_sample(data: &[u8], timestamp_us: u64) -> Result<IMFSample, BoxError> {
    let buffer = unsafe { MFCreateMemoryBuffer(data.len() as u32) }
        .map_err(|e| mf("MFCreateMemoryBuffer", e))?;

    let mut dst: *mut u8 = ptr::null_mut();
    unsafe { buffer.Lock(&mut dst, None, None) }.map_err(|e| mf("Lock", e))?;
    // SAFETY: o buffer tem data.len() bytes e fica travado até o Unlock abaixo
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        let _ = buffer.Unlock();
        buffer
            .SetCurrentLength(data.len() as u32)
            .map_err(|e| mf("SetCurrentLength", e))?;
    }

    let sample = unsafe { MFCreateSample() }.map_err(|e| mf("MFCreateSample", e))?;
    unsafe {
        sample.AddBuffer(&buffer).map_err(|e| mf("AddBuffer", e))?;
        // O Media Foundation conta o tempo em unidades de 100 ns
        sample
            .SetSampleTime(timestamp_us as i64 * 10)
            .map_err(|e| mf("SetSampleTime", e))?;
    }
    Ok(sample)
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
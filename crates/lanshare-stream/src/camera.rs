//! Captura da webcam pelo Media Foundation (Source Reader).
//!
//! - `list_cameras`: as câmeras do Windows (nome + id do dispositivo);
//! - `camera_modes`: os formatos que uma câmera entrega (resolução, FPS e formato: YUY2, NV12, MJPG...);
//! - `CameraCapture`: abre a câmera numa thread própria, escolhe o formato mais perto do pedido e
//!   entrega os frames já em RGBA (`CapturedFrame`, o mesmo da captura de tela), prontos para
//!   o encoder e para a prévia.
//!
//! O Source Reader faz a conversão (e a descompressão do MJPG) para RGB32 sozinho
//! (MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING).

use crate::capture::CapturedFrame;
use lanshare_core::i18n::tr;
use std::collections::BTreeSet;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

type BoxError = Box<dyn Error + Send + Sync>;

/// O primeiro fluxo de vídeo da câmera.
const VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Uma câmera do Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraDevice {
    /// Identificador do dispositivo (symbolic link). É o que se guarda nas Configurações.
    pub id: String,
    /// Nome que o Windows mostra (ex.: "HD Pro Webcam C920").
    pub name: String,
}

/// Um formato que a câmera entrega.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraMode {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    /// Formato dos pixels na câmera: "YUY2", "NV12", "MJPG", "H264"...
    pub format: String,
}

impl CameraMode {
    pub fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den.max(1) as f64
    }

    /// FPS arredondado (29.97 -> 30).
    pub fn fps_rounded(&self) -> u32 {
        self.fps().round() as u32
    }

    /// O formato chega sem compressão? (Não precisa descomprimir: menos trabalho para a CPU.)
    fn uncompressed(&self) -> bool {
        !matches!(self.format.as_str(), "MJPG" | "H264" | "HEVC")
    }
}

/// O que pedir à câmera. Ela usa o formato mais perto disso que tiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraRequest {
    pub height: u32,
    pub fps: u32,
}

// ───────────────────────── Listar câmeras e formatos ─────────────────────────

/// As câmeras ligadas no PC, na ordem do Windows.
pub fn list_cameras() -> Result<Vec<CameraDevice>, BoxError> {
    let _mf = MfRuntime::new()?;
    Ok(enum_devices()?
        .iter()
        .map(|activate| CameraDevice {
            id: read_string(activate, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)
                .unwrap_or_default(),
            name: read_string(activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                .unwrap_or_else(|| tr("Câmera", "Camera", "Cámara").to_string()),
        })
        .collect())
}

/// Os formatos que a câmera entrega (`None` = a primeira câmera), sem repetições,
/// do maior para o menor.
pub fn camera_modes(device_id: Option<&str>) -> Result<Vec<CameraMode>, BoxError> {
    let _mf = MfRuntime::new()?;
    let (source, reader, _) = open_reader(device_id)?;

    let mut seen = BTreeSet::new();
    let mut modes: Vec<CameraMode> = native_modes(&reader)
        .into_iter()
        .map(|(mode, _)| mode)
        .filter(|m| seen.insert((m.width, m.height, m.fps_num, m.fps_den, m.format.clone())))
        .collect();
    modes.sort_by(|a, b| {
        (b.height, b.width, b.fps_rounded()).cmp(&(a.height, a.width, a.fps_rounded()))
    });

    drop(reader);
    unsafe {
        let _ = source.Shutdown();
    }
    Ok(modes)
}

// ───────────────────────── Captura ─────────────────────────

/// Câmera ligada. Os frames (RGBA) chegam em `frames`; se o consumidor atrasar, os
/// mais antigos são descartados. Parar: `stop()` (ou soltar o valor).
pub struct CameraCapture {
    pub frames: Receiver<CapturedFrame>,
    /// O formato que a câmera está usando de verdade.
    pub mode: CameraMode,
    /// Nome da câmera.
    pub name: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CameraCapture {
    /// Liga a câmera (`None` = a primeira) com o formato mais perto de `request`.
    /// Só volta depois de a câmera abrir (ou dar erro).
    pub fn start(device_id: Option<String>, request: CameraRequest) -> Result<Self, BoxError> {
        let (frame_tx, frames) = mpsc::sync_channel(2);
        let (ready_tx, ready_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let thread = std::thread::Builder::new()
            .name("lanshare-camera".into())
            .spawn(move || run_camera(device_id, request, frame_tx, ready_tx, stop_thread))?;

        match ready_rx.recv() {
            Ok(Ok((mode, name))) => Ok(Self {
                frames,
                mode,
                name,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(erro)) => {
                let _ = thread.join();
                Err(erro)
            }
            Err(_) => {
                let _ = thread.join();
                Err(tr(
                    "A câmera fechou antes de abrir.",
                    "The camera closed before opening.",
                    "La cámara se cerró antes de abrirse.",
                )
                .into())
            }
        }
    }

    /// A câmera ainda está entregando imagens? (false se foi desconectada ou deu erro.)
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Desliga a câmera e espera a thread terminar.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A thread da câmera: abre, avisa `ready` e lê frames até `stop` (ou até a câmera sumir).
fn run_camera(
    device_id: Option<String>,
    request: CameraRequest,
    frames: SyncSender<CapturedFrame>,
    ready: Sender<Result<(CameraMode, String), BoxError>>,
    stop: Arc<AtomicBool>,
) {
    let _mf = match MfRuntime::new() {
        Ok(runtime) => runtime,
        Err(erro) => {
            let _ = ready.send(Err(erro));
            return;
        }
    };

    let (source, reader, mode, name, stride) = match open_camera(device_id.as_deref(), request) {
        Ok(opened) => opened,
        Err(erro) => {
            let _ = ready.send(Err(erro));
            return;
        }
    };
    let _ = ready.send(Ok((mode.clone(), name)));

    while !stop.load(Ordering::Relaxed) {
        let mut flags = 0u32;
        let mut sample: Option<IMFSample> = None;
        let read = unsafe {
            reader.ReadSample(
                VIDEO_STREAM,
                0,
                None,
                Some(&mut flags),
                None,
                Some(&mut sample),
            )
        };

        // Câmera desconectada ou com erro: encerra (o is_running passa a dar false)
        let ended = (MF_SOURCE_READERF_ERROR.0 | MF_SOURCE_READERF_ENDOFSTREAM.0) as u32;
        if read.is_err() || flags & ended != 0 {
            break;
        }

        let captured_at = Instant::now();
        let Some(sample) = sample else {
            continue; // aviso sem imagem (ex.: troca de formato)
        };

        let mut data = Vec::new();
        if copy_rgba(&sample, mode.width, mode.height, stride, &mut data).is_err() {
            continue;
        }

        match frames.try_send(CapturedFrame {
            width: mode.width,
            height: mode.height,
            data,
            captured_at,
        }) {
            Ok(()) | Err(TrySendError::Full(_)) => {} // cheio: descarta o frame
            Err(TrySendError::Disconnected(_)) => break, // ninguém está ouvindo mais
        }
    }

    drop(reader);
    unsafe {
        let _ = source.Shutdown();
    }
}

/// Abre a câmera, escolhe o formato e pede a saída em RGB32.
/// Devolve também a distância em bytes de uma linha para a outra (negativa = de baixo para cima).
fn open_camera(
    device_id: Option<&str>,
    request: CameraRequest,
) -> Result<(IMFMediaSource, IMFSourceReader, CameraMode, String, i32), BoxError> {
    let (source, reader, name) = open_reader(device_id)?;

    match configure_reader(&reader, request) {
        Ok((mode, stride)) => Ok((source, reader, mode, name, stride)),
        Err(erro) => {
            drop(reader);
            unsafe {
                let _ = source.Shutdown();
            }
            Err(erro)
        }
    }
}

fn configure_reader(reader: &IMFSourceReader, request: CameraRequest) -> Result<(CameraMode, i32), BoxError> {
    let (mode, native) = pick_mode(native_modes(reader), request).ok_or_else(|| -> BoxError {
        tr(
            "A câmera não informou nenhum formato de imagem.",
            "The camera did not report any image format.",
            "La cámara no informó ningún formato de imagen.",
        )
        .into()
    })?;

    unsafe {
        // 1) O formato da câmera (resolução/FPS)
        reader
            .SetCurrentMediaType(VIDEO_STREAM, None, &native)
            .map_err(|e| mf("formato da câmera", e))?;

        // 2) A saída em RGB32, no mesmo tamanho: o Source Reader converte/descomprime
        let output = MFCreateMediaType().map_err(|e| mf("MFCreateMediaType", e))?;
        output
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| mf("saída: tipo", e))?;
        output
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(|e| mf("saída: formato", e))?;
        output
            .SetUINT64(&MF_MT_FRAME_SIZE, pack_2x32(mode.width, mode.height))
            .map_err(|e| mf("saída: tamanho", e))?;
        output
            .SetUINT64(&MF_MT_FRAME_RATE, pack_2x32(mode.fps_num, mode.fps_den))
            .map_err(|e| mf("saída: FPS", e))?;
        reader
            .SetCurrentMediaType(VIDEO_STREAM, None, &output)
            .map_err(|e| mf("conversão para RGB32", e))?;

        // Distância entre as linhas da imagem que vai chegar
        let current = reader
            .GetCurrentMediaType(VIDEO_STREAM)
            .map_err(|e| mf("GetCurrentMediaType", e))?;
        let stride = current
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|s| s as i32)
            .unwrap_or((mode.width * 4) as i32);

        Ok((mode, stride))
    }
}

/// Acha a câmera (`None` = a primeira), ativa e cria o Source Reader dela.
fn open_reader(device_id: Option<&str>) -> Result<(IMFMediaSource, IMFSourceReader, String), BoxError> {
    let devices = enum_devices()?;
    let activate = match device_id {
        Some(id) => devices.iter().find(|a| {
            read_string(a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).as_deref() == Some(id)
        }),
        None => devices.first(),
    }
    .ok_or_else(|| -> BoxError {
        if device_id.is_some() {
            tr(
                "A câmera escolhida não foi encontrada (foi desconectada?).",
                "The chosen camera was not found (was it unplugged?).",
                "No se encontró la cámara elegida (¿se desconectó?).",
            )
        } else {
            tr(
                "Nenhuma câmera encontrada.",
                "No camera found.",
                "No se encontró ninguna cámara.",
            )
        }
        .into()
    })?;

    let name = read_string(activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).unwrap_or_default();

    unsafe {
        let source: IMFMediaSource = activate
            .ActivateObject()
            .map_err(|e| mf("abrir a câmera (está em uso por outro app?)", e))?;

        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1).map_err(|e| mf("MFCreateAttributes", e))?;
        let attributes = attributes.ok_or("MFCreateAttributes")?;
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
            .map_err(|e| mf("conversão de vídeo", e))?;

        match MFCreateSourceReaderFromMediaSource(&source, &attributes) {
            Ok(reader) => Ok((source, reader, name)),
            Err(erro) => {
                let _ = source.Shutdown();
                Err(mf("MFCreateSourceReaderFromMediaSource", erro))
            }
        }
    }
}

/// Os dispositivos de captura de vídeo do Windows.
fn enum_devices() -> Result<Vec<IMFActivate>, BoxError> {
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1).map_err(|e| mf("MFCreateAttributes", e))?;
        let attributes = attributes.ok_or("MFCreateAttributes")?;
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|e| mf("tipo de dispositivo", e))?;

        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attributes, &mut list, &mut count)
            .map_err(|e| mf("MFEnumDeviceSources", e))?;

        let mut devices = Vec::with_capacity(count as usize);
        if !list.is_null() {
            for i in 0..count as usize {
                if let Some(activate) = (*list.add(i)).take() {
                    devices.push(activate);
                }
            }
            CoTaskMemFree(Some(list as *const _));
        }
        Ok(devices)
    }
}

/// Todos os formatos que a câmera oferece (com o tipo de mídia de cada um, para escolher depois).
fn native_modes(reader: &IMFSourceReader) -> Vec<(CameraMode, IMFMediaType)> {
    let mut modes = Vec::new();
    let mut index = 0;
    while let Ok(media) = unsafe { reader.GetNativeMediaType(VIDEO_STREAM, index) } {
        if let Some(mode) = mode_of(&media) {
            modes.push((mode, media));
        }
        index += 1;
    }
    modes
}

fn mode_of(media: &IMFMediaType) -> Option<CameraMode> {
    unsafe {
        let size = media.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let rate = media.GetUINT64(&MF_MT_FRAME_RATE).ok()?;
        let subtype = media.GetGUID(&MF_MT_SUBTYPE).ok()?;

        let (width, height) = ((size >> 32) as u32, size as u32);
        let (fps_num, fps_den) = ((rate >> 32) as u32, (rate as u32).max(1));
        if width == 0 || height == 0 || fps_num == 0 {
            return None;
        }

        Some(CameraMode {
            width,
            height,
            fps_num,
            fps_den,
            format: format_name(&subtype),
        })
    }
}

/// O formato mais perto do pedido: a maior altura até a pedida; depois o FPS mais perto
/// (até o pedido); depois a imagem mais larga (16:9 antes de 4:3); por fim, sem compressão.
/// Se a câmera só tiver imagens maiores que o pedido, usa a menor delas.
fn pick_mode(
    modes: Vec<(CameraMode, IMFMediaType)>,
    request: CameraRequest,
) -> Option<(CameraMode, IMFMediaType)> {
    let fits = modes.iter().any(|(m, _)| m.height <= request.height);

    modes
        .into_iter()
        .filter(|(m, _)| !fits || m.height <= request.height)
        .max_by_key(|(m, _)| {
            // Sem nenhum que caiba: quanto menor a altura, melhor
            let height_score = if fits { m.height as i64 } else { -(m.height as i64) };
            (
                height_score,
                m.fps_rounded().min(request.fps),
                m.width,
                m.uncompressed(),
            )
        })
}

/// Copia a imagem RGB32 (BGRX) da câmera para `out` em RGBA, de cima para baixo e sem sobras.
fn copy_rgba(
    sample: &IMFSample,
    width: u32,
    height: u32,
    default_stride: i32,
    out: &mut Vec<u8>,
) -> Result<(), BoxError> {
    let (w, h) = (width as usize, height as usize);
    out.resize(w * h * 4, 0);

    unsafe {
        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|e| mf("ConvertToContiguousBuffer", e))?;

        // O jeito certo: o buffer 2D já diz onde começa a linha de cima e o passo entre linhas
        if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer>() {
            let mut scan0 = std::ptr::null_mut();
            let mut pitch = 0i32;
            buffer_2d
                .Lock2D(&mut scan0, &mut pitch)
                .map_err(|e| mf("Lock2D", e))?;
            bgrx_to_rgba(scan0, pitch as isize, w, h, out);
            let _ = buffer_2d.Unlock2D();
            return Ok(());
        }

        // Plano B: buffer comum, com o passo do tipo de mídia
        let mut data = std::ptr::null_mut();
        let mut length = 0u32;
        buffer
            .Lock(&mut data, None, Some(&mut length))
            .map_err(|e| mf("Lock", e))?;

        let stride = default_stride as isize;
        let row_bytes = stride.unsigned_abs();
        if row_bytes < w * 4 || (length as usize) < row_bytes * h {
            let _ = buffer.Unlock();
            return Err("buffer da câmera menor que a imagem".into());
        }

        // Passo negativo: a primeira linha guardada é a de baixo
        let top = if stride < 0 { data.add((h - 1) * row_bytes) } else { data };
        bgrx_to_rgba(top, stride, w, h, out);
        let _ = buffer.Unlock();
        Ok(())
    }
}

/// `top` aponta para a linha de cima; `pitch` é a distância (em bytes, pode ser negativa)
/// até a linha seguinte.
unsafe fn bgrx_to_rgba(top: *const u8, pitch: isize, width: usize, height: usize, out: &mut [u8]) {
    let row_len = width * 4;
    for y in 0..height {
        let src = std::slice::from_raw_parts(top.offset(y as isize * pitch), row_len);
        let dst = &mut out[y * row_len..(y + 1) * row_len];
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = 255;
        }
    }
}

// ───────────────────────── Auxiliares ─────────────────────────

/// Liga o COM e o Media Foundation enquanto existir; desliga ao sair (cada thread o seu).
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

/// Lê um texto do dispositivo (nome, id).
fn read_string(activate: &IMFActivate, key: &GUID) -> Option<String> {
    unsafe {
        let mut value = PWSTR::null();
        let mut length = 0u32;
        activate.GetAllocatedString(key, &mut value, &mut length).ok()?;
        let text = value.to_string().ok();
        CoTaskMemFree(Some(value.0 as *const _));
        text
    }
}

/// Nome curto do formato: o FourCC ("YUY2", "NV12", "MJPG"...) ou "RGB32"/"RGB24".
fn format_name(subtype: &GUID) -> String {
    if *subtype == MFVideoFormat_RGB32 {
        return "RGB32".into();
    }
    if *subtype == MFVideoFormat_RGB24 {
        return "RGB24".into();
    }

    let bytes = subtype.data1.to_le_bytes();
    if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        String::from_utf8_lossy(&bytes).trim().to_string()
    } else {
        format!("{:08X}", subtype.data1)
    }
}

fn mf(what: &str, erro: windows::core::Error) -> BoxError {
    format!("{what}: {erro}").into()
}

/// Dois números de 32 bits num só de 64 (formato que o Media Foundation usa).
fn pack_2x32(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

// ───────────────────────── Teste ─────────────────────────

/// Teste temporário: lista as câmeras e os formatos da primeira, liga ela (pedindo 1080p a
/// 30 FPS) por alguns segundos e grava o último frame em lanshare-camtest.bmp.
pub fn test_camera(seconds: u64) -> Result<(), BoxError> {
    let devices = list_cameras()?;
    if devices.is_empty() {
        return Err("nenhuma câmera encontrada".into());
    }

    println!("Câmeras encontradas:");
    for (i, device) in devices.iter().enumerate() {
        println!("  {}. {}", i + 1, device.name);
    }

    let camera = &devices[0];
    let modes = camera_modes(Some(&camera.id))?;
    println!("Formatos de \"{}\" ({}):", camera.name, modes.len());
    for mode in &modes {
        println!(
            "  {}x{} @ {:.2} FPS ({})",
            mode.width,
            mode.height,
            mode.fps(),
            mode.format
        );
    }

    let request = CameraRequest { height: 1080, fps: 30 };
    let capture = CameraCapture::start(Some(camera.id.clone()), request)?;
    println!(
        "Capturando {}x{} @ {:.0} FPS ({}) por {}s...",
        capture.mode.width,
        capture.mode.height,
        capture.mode.fps(),
        capture.mode.format,
        seconds
    );

    let start = Instant::now();
    let mut count = 0u32;
    let mut last: Option<CapturedFrame> = None;
    while start.elapsed() < Duration::from_secs(seconds) {
        if let Ok(frame) = capture.frames.recv_timeout(Duration::from_millis(500)) {
            count += 1;
            last = Some(frame);
        }
        if !capture.is_running() {
            println!("A câmera parou de entregar imagens (desconectada?).");
            break;
        }
    }
    capture.stop();

    println!(
        "Frames recebidos: {} em {}s (~{:.1} FPS)",
        count,
        seconds,
        count as f64 / seconds.max(1) as f64
    );

    if let Some(frame) = last {
        save_bmp("lanshare-camtest.bmp", &frame)?;
        println!("Último frame salvo em lanshare-camtest.bmp");
    }
    Ok(())
}

/// Grava um frame RGBA como .bmp de 32 bits (de cima para baixo), para conferir a imagem.
fn save_bmp(path: &str, frame: &CapturedFrame) -> Result<(), BoxError> {
    const HEADER: u32 = 14 + 40;
    let pixels = frame.width * frame.height * 4;

    let mut bytes = Vec::with_capacity((HEADER + pixels) as usize);
    // Cabeçalho do arquivo
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&(HEADER + pixels).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&HEADER.to_le_bytes());
    // Cabeçalho da imagem (altura negativa = de cima para baixo)
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(frame.width as i32).to_le_bytes());
    bytes.extend_from_slice(&(-(frame.height as i32)).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // sem compressão
    bytes.extend_from_slice(&pixels.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 16]);
    // Pixels: o BMP guarda BGRA
    for px in frame.data.chunks_exact(4) {
        bytes.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }

    std::fs::write(path, bytes)?;
    Ok(())
}
//! Captura do som do Host via WASAPI "process loopback" (Windows 10 2004+ / Windows 11).
//!
//! Captura tudo o que toca no PC, MENOS o próprio LANShare (e os processos filhos dele):
//! - os sons de notificação do app não vão para a transmissão;
//! - com Host e Client no mesmo PC, o som que o Client toca não volta para a captura (sem microfonia).
//!
//! Sai: blocos de PCM 16 bits, 48 kHz, estéreo (o formato que o Opus usa).

use std::error::Error;
use std::ffi::c_void;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lanshare_core::i18n::tr;
use windows::core::{implement, w, IUnknown, Interface, Ref, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

type BoxError = Box<dyn Error + Send + Sync>;

/// Formato da captura: o mesmo que o Opus vai receber.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
const BITS_PER_SAMPLE: u16 = 16;
const BLOCK_ALIGN: u16 = CHANNELS * BITS_PER_SAMPLE / 8;

/// Tamanho do buffer do Windows, em unidades de 100 ns (20 ms).
const BUFFER_HNS: i64 = 200_000;

/// Quantos blocos podem esperar na fila (cada bloco tem ~10 ms). Passou disso, descarta.
const QUEUE_SIZE: usize = 100;

/// Tempo máximo esperando o Windows ativar a captura.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);

/// De quanto em quanto tempo a thread confere se deve parar (quando não chega som).
const WAIT_MS: u32 = 100;

// Constantes do WASAPI (audioclient.h / audioclientactivationparams.h)
const VT_BLOB: u16 = 65;
const ACTIVATION_TYPE_PROCESS_LOOPBACK: i32 = 1;
const LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE: i32 = 1;
const STREAMFLAGS_LOOPBACK: u32 = 0x0002_0000;
const STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;
const STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
const BUFFERFLAGS_SILENT: u32 = 0x2;
const BUFFERFLAGS_TIMESTAMP_ERROR: u32 = 0x4;
const WAVE_FORMAT_PCM: u16 = 1;

/// Um bloco de som capturado.
pub struct AudioChunk {
    /// Amostras intercaladas (esquerda, direita, esquerda, direita...).
    pub samples: Vec<i16>,
    /// Quando a primeira amostra do bloco tocou no PC (mesmo relógio do vídeo).
    pub captured_at: Instant,
    /// true = o Windows avisou que o bloco é silêncio.
    pub silent: bool,
}

/// Captura de som em andamento. Os blocos chegam em `chunks`.
pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), String>>>,
    pub chunks: Receiver<AudioChunk>,
}

impl AudioCapture {
    /// Começa a capturar o som do PC (menos o do LANShare) numa thread própria.
    /// Só volta depois que a captura começou de verdade (ou falhou).
    pub fn start() -> Result<Self, BoxError> {
        let (sender, chunks) = mpsc::sync_channel(QUEUE_SIZE);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        let thread = std::thread::Builder::new()
            .name("lanshare-audio-capture".to_string())
            .spawn(move || run(sender, thread_stop, ready_tx))?;

        match ready_rx.recv_timeout(ACTIVATE_TIMEOUT + Duration::from_secs(1)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
                chunks,
            }),
            Ok(Err(erro)) => {
                let _ = thread.join();
                Err(erro.into())
            }
            Err(_) => {
                // Travou ao iniciar: pede para parar e não espera (a thread termina sozinha)
                stop.store(true, Ordering::Relaxed);
                Err(tr(
                    "o Windows demorou demais para iniciar a captura de áudio",
                    "Windows took too long to start the audio capture",
                    "Windows tardó demasiado en iniciar la captura de audio",
                )
                .into())
            }
        }
    }

    /// Para a captura e espera a thread terminar. Devolve o erro, se a captura caiu sozinha.
    pub fn stop(mut self) -> Result<(), BoxError> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take().map(|t| t.join()) {
            Some(Ok(Err(erro))) => Err(erro.into()),
            Some(Err(_)) => Err("a thread de captura de audio terminou com panico".into()),
            _ => Ok(()),
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Liga o COM nesta thread enquanto existir.
struct ComGuard;

impl ComGuard {
    fn new() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|e| format!("CoInitializeEx: {e}"))?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Evento do Windows que avisa "chegou som". Fechado ao sair.
struct EventHandle(HANDLE);

impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// A thread da captura: prepara tudo, avisa em `ready` e fica lendo o som até pedirem para parar.
fn run(
    sender: SyncSender<AudioChunk>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let prepared = ComGuard::new().and_then(|com| {
        let (client, capture, event) = open_capture()?;
        Ok((com, client, capture, event))
    });

    let (_com, client, capture, event) = match prepared {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(erro) => {
            let message = format!(
                "{}: {erro}",
                tr(
                    "Não foi possível iniciar a captura de áudio",
                    "Could not start the audio capture",
                    "No se pudo iniciar la captura de audio"
                )
            );
            let _ = ready.send(Err(message.clone()));
            return Err(message);
        }
    };

    let clock = QpcClock::new();
    let result = read_loop(&capture, &event, &sender, &stop, &clock);

    unsafe {
        let _ = client.Stop();
    }
    result
}

/// Ativa o "process loopback" e deixa a captura pronta e rodando.
fn open_capture() -> Result<(IAudioClient, IAudioCaptureClient, EventHandle), String> {
    let client = activate_process_loopback()?;

    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM,
        nChannels: CHANNELS,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * BLOCK_ALIGN as u32,
        nBlockAlign: BLOCK_ALIGN,
        wBitsPerSample: BITS_PER_SAMPLE,
        cbSize: 0,
    };

    unsafe {
        // O process loopback não tem "formato do dispositivo": nós escolhemos e o Windows converte
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                STREAMFLAGS_LOOPBACK | STREAMFLAGS_EVENTCALLBACK | STREAMFLAGS_AUTOCONVERTPCM,
                BUFFER_HNS,
                0,
                &format,
                None,
            )
            .map_err(|e| format!("Initialize: {e}"))?;

        let event = EventHandle(
            CreateEventW(None, false, false, PCWSTR::null()).map_err(|e| format!("CreateEvent: {e}"))?,
        );
        client
            .SetEventHandle(event.0)
            .map_err(|e| format!("SetEventHandle: {e}"))?;

        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|e| format!("GetService: {e}"))?;

        client.Start().map_err(|e| format!("Start: {e}"))?;
        Ok((client, capture, event))
    }
}

/// Lê os pacotes de som que o Windows entrega até pedirem para parar.
fn read_loop(
    capture: &IAudioCaptureClient,
    event: &EventHandle,
    sender: &SyncSender<AudioChunk>,
    stop: &AtomicBool,
    clock: &QpcClock,
) -> Result<(), String> {
    while !stop.load(Ordering::Relaxed) {
        // Espera o aviso de "chegou som" (com limite, para conferir o `stop`)
        if unsafe { WaitForSingleObject(event.0, WAIT_MS) } != WAIT_OBJECT_0 {
            continue;
        }

        loop {
            let packet = unsafe { capture.GetNextPacketSize() }
                .map_err(|e| format!("GetNextPacketSize: {e}"))?;
            if packet == 0 {
                break;
            }

            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames: u32 = 0;
            let mut flags: u32 = 0;
            let mut qpc_position: u64 = 0;
            unsafe {
                capture
                    .GetBuffer(
                        &mut data,
                        &mut frames,
                        &mut flags,
                        None,
                        Some(&mut qpc_position as *mut u64),
                    )
                    .map_err(|e| format!("GetBuffer: {e}"))?;
            }

            let sample_count = frames as usize * CHANNELS as usize;
            let silent = flags & BUFFERFLAGS_SILENT != 0;
            let samples = if silent || data.is_null() {
                vec![0i16; sample_count]
            } else {
                // SAFETY: o Windows garante `frames` quadros de BLOCK_ALIGN bytes até o ReleaseBuffer
                let bytes =
                    unsafe { std::slice::from_raw_parts(data, frames as usize * BLOCK_ALIGN as usize) };
                bytes
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect()
            };

            unsafe { capture.ReleaseBuffer(frames) }.map_err(|e| format!("ReleaseBuffer: {e}"))?;

            if frames == 0 {
                continue;
            }

            let timestamp_ok = flags & BUFFERFLAGS_TIMESTAMP_ERROR == 0 && qpc_position != 0;
            let chunk = AudioChunk {
                samples,
                captured_at: clock.instant_of(timestamp_ok.then_some(qpc_position), frames),
                silent,
            };

            match sender.try_send(chunk) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {} // quem lê está atrasado: descarta o bloco
                Err(TrySendError::Disconnected(_)) => return Ok(()), // ninguém mais ouvindo
            }
        }
    }
    Ok(())
}

// ───────────────────────────── Ativação do process loopback ─────────────────────────────

/// Parâmetros do process loopback (AUDIOCLIENT_ACTIVATION_PARAMS).
#[repr(C)]
struct ProcessLoopbackParams {
    activation_type: i32,
    target_process_id: u32,
    loopback_mode: i32,
}

/// Um PROPVARIANT do tipo VT_BLOB, no mesmo formato de memória do Windows (24 bytes no x64).
#[repr(C)]
struct BlobPropVariant {
    vt: u16,
    reserved: [u16; 3],
    size: u32,
    data: *const c_void,
}

/// Recebe o aviso do Windows de que a ativação terminou.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivationHandler {
    done: mpsc::Sender<()>,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.done.send(());
        Ok(())
    }
}

/// Pede ao Windows um IAudioClient que captura tudo, menos este processo (e os filhos dele).
fn activate_process_loopback() -> Result<IAudioClient, String> {
    let params = ProcessLoopbackParams {
        activation_type: ACTIVATION_TYPE_PROCESS_LOOPBACK,
        target_process_id: std::process::id(),
        loopback_mode: LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    };
    let prop = BlobPropVariant {
        vt: VT_BLOB,
        reserved: [0; 3],
        size: std::mem::size_of::<ProcessLoopbackParams>() as u32,
        data: &params as *const ProcessLoopbackParams as *const c_void,
    };

    let (done_tx, done_rx) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler { done: done_tx }.into();

    // `params` e `prop` ficam vivos até o fim desta função (depois que a ativação termina)
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            w!("VAD\\Process_Loopback"),
            &IAudioClient::IID,
            Some(&prop as *const BlobPropVariant as *const _),
            &handler,
        )
    }
    .map_err(|e| format!("ActivateAudioInterfaceAsync: {e}"))?;

    done_rx
        .recv_timeout(ACTIVATE_TIMEOUT)
        .map_err(|_| "a ativacao do process loopback nao respondeu".to_string())?;

    let mut activate_result = HRESULT(0);
    let mut activated: Option<IUnknown> = None;
    unsafe { operation.GetActivateResult(&mut activate_result, &mut activated) }
        .map_err(|e| format!("GetActivateResult: {e}"))?;
    activate_result
        .ok()
        .map_err(|e| format!("process loopback: {e}"))?;

    activated
        .ok_or_else(|| "o Windows nao devolveu o cliente de audio".to_string())?
        .cast::<IAudioClient>()
        .map_err(|e| format!("IAudioClient: {e}"))
}

// ───────────────────────────── Relógio ─────────────────────────────

/// Converte o horário que o Windows marca em cada pacote (QPC) para `Instant`,
/// o mesmo relógio que o vídeo usa. Isso vai ser a base da sincronia entre som e imagem.
struct QpcClock {
    frequency: i64,
}

impl QpcClock {
    fn new() -> Self {
        let mut frequency = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut frequency);
        }
        Self { frequency }
    }

    /// Quando o pacote tocou. Sem horário válido, estima pelo tamanho do pacote.
    fn instant_of(&self, qpc_position_hns: Option<u64>, frames: u32) -> Instant {
        let now = Instant::now();
        let packet = Duration::from_micros(frames as u64 * 1_000_000 / SAMPLE_RATE as u64);

        let age = match (qpc_position_hns, self.now_hns()) {
            (Some(position), Some(current)) => {
                // Unidades de 100 ns; limita a 1 s para nunca gerar um horário absurdo
                let hns = (current - position as i64).clamp(0, 10_000_000);
                Duration::from_nanos(hns as u64 * 100)
            }
            _ => packet,
        };

        now.checked_sub(age).unwrap_or(now)
    }

    /// Agora, em unidades de 100 ns do QPC.
    fn now_hns(&self) -> Option<i64> {
        if self.frequency <= 0 {
            return None;
        }
        let mut counter = 0i64;
        unsafe { QueryPerformanceCounter(&mut counter) }.ok()?;
        Some((counter as i128 * 10_000_000 / self.frequency as i128) as i64)
    }
}

// ───────────────────────────── Teste (comando `audiotest`) ─────────────────────────────

/// Teste temporário: captura o som por alguns segundos, mostra o resultado e grava um .wav.
pub fn test_audio_capture(seconds: u64) -> Result<(), BoxError> {
    let capture = AudioCapture::start()?;
    println!("[audio] Captura iniciada. Toque algum som no PC (o som do LANShare fica de fora).");

    let start = Instant::now();
    let mut samples: Vec<i16> = Vec::new();
    let mut packets: u32 = 0;
    let mut silent_packets: u32 = 0;
    let mut delay_total = Duration::ZERO;

    while start.elapsed() < Duration::from_secs(seconds) {
        let Ok(chunk) = capture.chunks.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        packets += 1;
        if chunk.silent {
            silent_packets += 1;
        }
        delay_total += chunk.captured_at.elapsed();
        samples.extend_from_slice(&chunk.samples);
    }

    capture.stop()?;

    let captured_secs = samples.len() as f64 / (SAMPLE_RATE as f64 * CHANNELS as f64);
    println!(
        "[audio] Formato: {} Hz, {} canais, {} bits",
        SAMPLE_RATE, CHANNELS, BITS_PER_SAMPLE
    );
    println!(
        "[audio] Som capturado: {captured_secs:.1}s em {seconds}s de teste ({packets} pacotes, {silent_packets} silenciosos)"
    );

    if packets == 0 {
        println!("[audio] Nenhum pacote chegou. Toque algum som durante o teste.");
        return Ok(());
    }

    println!(
        "[audio] Atraso medio da captura: {:.1} ms",
        delay_total.as_secs_f64() * 1000.0 / packets as f64
    );

    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    if peak == 0 {
        println!("[audio] Tudo em silencio: nenhum outro programa tocou som durante o teste.");
    } else {
        let dbfs = 20.0 * (peak as f64 / i16::MAX as f64).log10();
        println!("[audio] Pico do som: {dbfs:.1} dBFS");
    }

    if captured_secs < seconds as f64 * 0.9 {
        println!("[audio] Obs.: trechos sem som podem nao gerar pacotes; a transmissao vai preencher isso com silencio.");
    }

    let path = write_wav(&samples)?;
    println!("[audio] Arquivo salvo: {}", path.display());
    Ok(())
}

/// Grava as amostras num .wav (PCM 16 bits) na pasta atual.
fn write_wav(samples: &[i16]) -> io::Result<PathBuf> {
    let path = std::env::current_dir()?.join("lanshare-audiotest.wav");
    let data_len = (samples.len() * 2) as u32;

    let mut file = BufWriter::new(File::create(&path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&WAVE_FORMAT_PCM.to_le_bytes())?;
    file.write_all(&CHANNELS.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * BLOCK_ALIGN as u32).to_le_bytes())?;
    file.write_all(&BLOCK_ALIGN.to_le_bytes())?;
    file.write_all(&BITS_PER_SAMPLE.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()?;
    Ok(path)
}

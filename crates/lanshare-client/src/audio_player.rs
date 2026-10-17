//! Som da transmissão no Client: pacotes Opus -> PCM -> WASAPI, na saída escolhida nas Configurações.
//!
//! Roda numa thread própria. A saída segue a escolha do usuário em tempo real:
//! - "Padrão do Windows" (None): acompanha se o padrão do Windows mudar;
//! - um dispositivo específico: se ele for desconectado, o som vai para o padrão
//!   e volta para ele sozinho quando ele reaparecer.

use crate::audio_devices::{self, ComGuard};
use crate::engine::EventTx;
use lanshare_core::i18n::tr;
use lanshare_core::protocol::AudioFrame;
use opus::{Channels, Decoder};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    IAudioClient, IAudioRenderClient, IMMDevice, AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX,
};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// Formato do som (o mesmo do Host).
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const BLOCK_ALIGN: u16 = (CHANNELS * 2) as u16;

/// Quantos pacotes (20 ms cada) podem esperar a thread do som. Passou disso, descarta.
const QUEUE_SIZE: usize = 50;

/// Maior pacote Opus decodificado: 120 ms de estéreo.
const MAX_DECODED_SAMPLES: usize = 5760 * CHANNELS;

/// Buffer do Windows, em unidades de 100 ns (50 ms).
const BUFFER_HNS: i64 = 500_000;

/// Antes de começar a tocar (ou depois de faltar som), junta este tanto de som:
/// absorve as pequenas variações de tempo da rede sem picotar.
const START_LATENCY_MS: usize = 60;

/// Se o som acumulado passar disso, joga fora o excesso (senão o atraso só cresce).
const MAX_LATENCY_MS: usize = 200;

/// Depois de jogar fora o excesso, fica com este tanto.
const TRIM_LATENCY_MS: usize = 80;

/// De quanto em quanto tempo confere se a saída certa mudou (padrão do Windows, fone conectado...).
const DEVICE_CHECK: Duration = Duration::from_secs(1);

/// Depois de uma falha ao abrir a saída, espera isso antes de tentar de novo.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Quanto tempo a thread dorme esperando a saída pedir mais som.
const WAIT_MS: u32 = 20;

// Constantes do WASAPI (audioclient.h)
const STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;
const STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;
const STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
const WAVE_FORMAT_PCM: u16 = 1;

fn ms_to_frames(ms: usize) -> usize {
    SAMPLE_RATE as usize * ms / 1000
}

// ───────────────────────────── Saída escolhida ─────────────────────────────

/// A saída de áudio escolhida nas Configurações, compartilhada com a thread do som.
/// Clonar NÃO copia: todos os clones veem a mesma escolha.
#[derive(Clone, Default)]
pub struct AudioOutputSetting {
    /// (id do dispositivo; None = padrão do Windows, contador de mudanças)
    inner: Arc<Mutex<(Option<String>, u64)>>,
}

impl AudioOutputSetting {
    /// Troca a saída. Se já houver som tocando, ele muda de saída na hora.
    pub fn set(&self, id: Option<String>) {
        let id = id.filter(|text| !text.trim().is_empty());
        let mut data = self.lock();
        if data.0 != id {
            data.0 = id;
            data.1 += 1;
        }
    }

    fn get(&self) -> (Option<String>, u64) {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, (Option<String>, u64)> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// ───────────────────────────── Player ─────────────────────────────

/// A thread que toca o som. Ao ser descartada, a thread termina sozinha.
pub struct AudioPlayer {
    tx: SyncSender<AudioFrame>,
}

impl AudioPlayer {
    pub fn start(output: AudioOutputSetting, events: EventTx) -> Self {
        let (tx, rx) = sync_channel(QUEUE_SIZE);

        let _ = std::thread::Builder::new()
            .name("lanshare-audio-player".to_string())
            .spawn(move || run(rx, output, events));

        Self { tx }
    }

    /// Entrega um pacote. Nunca trava a conexão: se a fila estiver cheia, descarta.
    pub fn push(&self, frame: AudioFrame) {
        let _ = self.tx.try_send(frame);
    }
}

fn send_error(events: &EventTx, message: String) {
    let _ = events.send(json!({ "type": "audio_error", "error": message }));
}

fn run(rx: Receiver<AudioFrame>, output: AudioOutputSetting, events: EventTx) {
    // Primeiro a ser criado = último a ser liberado (o COM precisa sobreviver à saída)
    let _com = match ComGuard::new() {
        Ok(com) => com,
        Err(erro) => {
            send_error(&events, erro);
            return;
        }
    };

    let mut decoder = match Decoder::new(SAMPLE_RATE, Channels::Stereo) {
        Ok(decoder) => decoder,
        Err(erro) => {
            send_error(
                &events,
                format!(
                    "{}: {erro}",
                    tr(
                        "Não foi possível iniciar o som da transmissão",
                        "Could not start the stream audio",
                        "No se pudo iniciar el sonido de la transmisión"
                    )
                ),
            );
            return;
        }
    };

    let mut queue = PlaybackQueue::new();
    let mut pcm = vec![0i16; MAX_DECODED_SAMPLES];
    let mut device: Option<RenderDevice> = None;
    let mut seen_generation: Option<u64> = None;
    let mut next_check = Instant::now();
    let mut warned = false;

    loop {
        // 1) Decodifica tudo o que chegou
        loop {
            match rx.try_recv() {
                Ok(frame) => decode_into(&mut decoder, &frame, &mut pcm, &mut queue),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return, // a transmissão acabou
            }
        }

        // 2) Confere se a saída certa está aberta (mudou a escolha, mudou o padrão, caiu...)
        let (chosen, generation) = output.get();
        if seen_generation != Some(generation) || Instant::now() >= next_check {
            seen_generation = Some(generation);
            next_check = Instant::now() + DEVICE_CHECK;

            match switch_device(&mut device, chosen.as_deref()) {
                Ok(()) => warned = false,
                Err(erro) => {
                    next_check = Instant::now() + RETRY_DELAY;
                    if !warned {
                        warned = true;
                        send_error(&events, erro);
                    }
                }
            }
        }

        // 3) Entrega som à saída e espera ela pedir mais
        let fed = device
            .as_ref()
            .map(|open| open.feed(&mut queue).map(|()| open.event.0));

        match fed {
            Some(Ok(event)) => unsafe {
                let _ = WaitForSingleObject(event, WAIT_MS);
            },
            Some(Err(_)) => {
                // Saída sumiu (fone desconectado, driver reiniciado): reabre já
                device = None;
                next_check = Instant::now();
            }
            None => {
                // Sem saída: continua recebendo para o atraso não acumular
                match rx.recv_timeout(Duration::from_millis(WAIT_MS as u64)) {
                    Ok(frame) => decode_into(&mut decoder, &frame, &mut pcm, &mut queue),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

fn decode_into(decoder: &mut Decoder, frame: &AudioFrame, pcm: &mut [i16], queue: &mut PlaybackQueue) {
    match decoder.decode(&frame.data, pcm, false) {
        Ok(frames) => queue.push(&pcm[..frames * CHANNELS]),
        Err(_) => {
            // Pacote inválido: ignora (o próximo decodifica normalmente)
        }
    }
}

/// Abre a saída que deve tocar agora, se ela não for a que já está aberta.
fn switch_device(device: &mut Option<RenderDevice>, chosen: Option<&str>) -> Result<(), String> {
    let enumerator = audio_devices::enumerator().map_err(|e| format!("IMMDeviceEnumerator: {e}"))?;

    let Some((id, wanted)) = audio_devices::resolve_output(&enumerator, chosen) else {
        *device = None;
        return Err(tr(
            "Nenhuma saída de áudio encontrada neste PC.",
            "No audio output found on this PC.",
            "No se encontró ninguna salida de audio en este PC.",
        )
        .to_string());
    };

    if device.as_ref().is_some_and(|open| open.id == id) {
        return Ok(());
    }

    // Fecha a anterior antes de abrir a nova
    *device = None;
    *device = Some(RenderDevice::open(id, &wanted).map_err(|erro| {
        format!(
            "{}: {erro}",
            tr(
                "Não foi possível abrir a saída de áudio",
                "Could not open the audio output",
                "No se pudo abrir la salida de audio"
            )
        )
    })?);
    Ok(())
}

// ───────────────────────────── Fila de som ─────────────────────────────

/// O som já decodificado, esperando para tocar. Controla o atraso: junta um pouco antes de
/// começar e joga fora o excesso se acumular demais.
struct PlaybackQueue {
    samples: VecDeque<i16>,
    /// true = juntando som antes de tocar (no começo ou depois de faltar som).
    buffering: bool,
}

impl PlaybackQueue {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(ms_to_frames(MAX_LATENCY_MS) * CHANNELS * 2),
            buffering: true,
        }
    }

    fn frames(&self) -> usize {
        self.samples.len() / CHANNELS
    }

    fn push(&mut self, pcm: &[i16]) {
        self.samples.extend(pcm.iter().copied());

        // Atraso grande demais (a saída travou um pouco, ou a rede entregou de uma vez)
        if self.frames() > ms_to_frames(MAX_LATENCY_MS) {
            let excess = (self.frames() - ms_to_frames(TRIM_LATENCY_MS)) * CHANNELS;
            self.samples.drain(..excess);
        }
    }

    /// Quantos quadros podem ser tocados agora (no máximo `free`).
    fn ready(&mut self, free: usize) -> usize {
        if self.buffering {
            if self.frames() < ms_to_frames(START_LATENCY_MS) {
                return 0;
            }
            self.buffering = false;
        }

        let frames = self.frames().min(free);
        if frames == 0 {
            // Acabou o som: volta a juntar antes de tocar de novo
            self.buffering = true;
        }
        frames
    }

    /// Tira `frames` quadros da fila e grava em `out` (PCM 16 bits, little-endian).
    fn take_into(&mut self, frames: usize, out: &mut [u8]) {
        for (sample, bytes) in self.samples.drain(..frames * CHANNELS).zip(out.chunks_exact_mut(2)) {
            bytes.copy_from_slice(&sample.to_le_bytes());
        }
    }
}

// ───────────────────────────── Saída (WASAPI) ─────────────────────────────

/// Evento do Windows que avisa "a saída quer mais som". Fechado ao sair.
struct EventHandle(HANDLE);

impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Uma saída de áudio aberta e tocando.
struct RenderDevice {
    id: String,
    client: IAudioClient,
    render: IAudioRenderClient,
    event: EventHandle,
    buffer_frames: u32,
}

impl RenderDevice {
    fn open(id: String, device: &IMMDevice) -> Result<Self, String> {
        unsafe {
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("Activate: {e}"))?;

            // Nós entregamos PCM 16 bits / 48 kHz; o Windows converte para o formato da saída
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM,
                nChannels: CHANNELS as u16,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * BLOCK_ALIGN as u32,
                nBlockAlign: BLOCK_ALIGN,
                wBitsPerSample: 16,
                cbSize: 0,
            };

            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    STREAMFLAGS_EVENTCALLBACK
                        | STREAMFLAGS_AUTOCONVERTPCM
                        | STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER_HNS,
                    0,
                    &format,
                    None,
                )
                .map_err(|e| format!("Initialize: {e}"))?;

            let event = EventHandle(
                CreateEventW(None, false, false, PCWSTR::null())
                    .map_err(|e| format!("CreateEvent: {e}"))?,
            );
            client
                .SetEventHandle(event.0)
                .map_err(|e| format!("SetEventHandle: {e}"))?;

            let buffer_frames = client
                .GetBufferSize()
                .map_err(|e| format!("GetBufferSize: {e}"))?;
            let render: IAudioRenderClient = client
                .GetService()
                .map_err(|e| format!("GetService: {e}"))?;

            client.Start().map_err(|e| format!("Start: {e}"))?;

            Ok(Self {
                id,
                client,
                render,
                event,
                buffer_frames,
            })
        }
    }

    /// Entrega à saída o som que couber. Erro = a saída deixou de existir.
    fn feed(&self, queue: &mut PlaybackQueue) -> windows::core::Result<()> {
        unsafe {
            let padding = self.client.GetCurrentPadding()?;
            let free = self.buffer_frames.saturating_sub(padding) as usize;

            let frames = queue.ready(free);
            if frames == 0 {
                return Ok(());
            }

            let data = self.render.GetBuffer(frames as u32)?;
            // SAFETY: o Windows deu `frames` quadros de BLOCK_ALIGN bytes até o ReleaseBuffer
            let out = std::slice::from_raw_parts_mut(data, frames * BLOCK_ALIGN as usize);
            queue.take_into(frames, out);
            self.render.ReleaseBuffer(frames as u32, 0)?;
        }
        Ok(())
    }
}

impl Drop for RenderDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

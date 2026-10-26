//! Som do monitor inteiro SEM os apps bloqueados nas Configurações (Discord, Teams...).
//!
//! O Windows só deixa excluir um processo por captura. Então o mixer faz o contrário:
//! 1. a cada segundo, vê quais apps têm sessão de áudio aberta;
//! 2. abre uma captura (process loopback INCLUDE) para cada app permitido e fecha as que sobraram;
//! 3. junta tudo numa linha do tempo única e entrega blocos prontos, iguais aos de uma captura só.
//!
//! Cuidado com a "árvore de processos": capturar um processo inclui os filhos dele.
//! - Um app que é pai de um app bloqueado (ou do LANShare) não é capturado: senão a chamada vazaria.
//!   Ex.: o explorer.exe abriu o Discord; capturar o explorer levaria a voz junto.
//! - Um app filho de um app bloqueado também não (ex.: o WebView do Teams).
//! - Se pai e filho tocam som, só o pai é capturado (o filho já vem junto, sem som dobrado).
//!
//! A lista de apps bloqueados é relida a cada segundo: mudar nas Configurações vale na hora.

use crate::audio_capture::{AudioCapture, AudioChunk, AudioSource, CHANNELS, SAMPLE_RATE};
use crate::audio_sessions;
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

type BoxError = Box<dyn Error + Send + Sync>;

const CH: usize = CHANNELS as usize;

/// De quanto em quanto tempo confere quais apps estão com som.
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// De quanto em quanto tempo o mixer junta e entrega o som.
const TICK: Duration = Duration::from_millis(10);

/// O som fica esperando este tanto antes de sair: tempo para todas as capturas entregarem
/// a sua parte daquele trecho (cada uma chega num ritmo um pouco diferente).
const LATENCY_FRAMES: i64 = SAMPLE_RATE as i64 * 60 / 1000;

/// Se um bloco chega até este tanto fora do lugar esperado, é encaixado logo depois do anterior
/// do mesmo app (os horários do Windows variam um pouco; sem isso o som teria estalos).
const SNAP_FRAMES: i64 = SAMPLE_RATE as i64 * 5 / 1000;

/// Nada é guardado mais que 1 s à frente (protege contra um horário errado do Windows).
const MAX_AHEAD_FRAMES: usize = SAMPLE_RATE as usize;

/// Máximo de apps capturados ao mesmo tempo.
const MAX_SOURCES: usize = 16;

/// Quantos blocos prontos podem esperar quem envia pela rede.
const QUEUE_SIZE: usize = 100;

/// Limite ao subir a árvore de processos (protege contra um "pai" que aponta para trás).
const MAX_DEPTH: usize = 64;

/// O som misturado em andamento. Os blocos chegam em `chunks`, como numa AudioCapture.
pub struct MixedCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub chunks: Receiver<AudioChunk>,
}

impl MixedCapture {
    /// Começa a montar o som do PC sem os apps bloqueados, numa thread própria.
    pub fn start() -> Result<Self, BoxError> {
        let (sender, chunks) = mpsc::sync_channel(QUEUE_SIZE);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        let thread = std::thread::Builder::new()
            .name("lanshare-audio-mixer".to_string())
            .spawn(move || run(sender, &thread_stop))?;

        Ok(Self {
            stop,
            thread: Some(thread),
            chunks,
        })
    }

    /// Para tudo (as capturas de cada app também) e espera a thread terminar.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for MixedCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Liga o COM nesta thread enquanto existir (a lista de sessões de áudio precisa dele).
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// A captura de um app e onde o próximo bloco dele deve cair na linha do tempo.
struct Source {
    capture: AudioCapture,
    next_frame: Option<i64>,
}

fn run(sender: SyncSender<AudioChunk>, stop: &AtomicBool) {
    let _com = ComGuard::new();

    // Ponto zero da linha do tempo do mixer (em quadros de 48 kHz a partir daqui)
    let origin = Instant::now();
    let mut mix = MixBuffer::default();
    let mut sources: HashMap<u32, Source> = HashMap::new();
    let mut failed: HashSet<u32> = HashSet::new();
    let mut next_refresh = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_refresh {
            refresh(&mut sources, &mut failed);
            next_refresh = Instant::now() + REFRESH_INTERVAL;
        }

        // Junta o que cada app entregou
        let mut ended = Vec::new();
        for (pid, source) in sources.iter_mut() {
            loop {
                match source.capture.chunks.try_recv() {
                    Ok(chunk) => place(&mut mix, source, origin, chunk),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended.push(*pid); // a captura caiu (o app fechou)
                        break;
                    }
                }
            }
        }
        for pid in ended {
            if let Some(source) = sources.remove(&pid) {
                let _ = source.capture.stop();
            }
        }

        // Entrega o trecho que já teve tempo de receber a parte de todos
        let ready = frame_of(origin, Instant::now()) - LATENCY_FRAMES;
        if let Some((start, samples)) = mix.take_until(ready) {
            let chunk = AudioChunk {
                samples,
                captured_at: origin + frames_to_duration(start),
                silent: false,
            };
            if let Err(TrySendError::Disconnected(_)) = sender.try_send(chunk) {
                break; // ninguém mais ouvindo
            }
        }

        std::thread::sleep(TICK);
    }

    for (_, source) in sources.drain() {
        let _ = source.capture.stop();
    }
}

/// Coloca um bloco de um app na linha do tempo.
fn place(mix: &mut MixBuffer, source: &mut Source, origin: Instant, chunk: AudioChunk) {
    let frames = (chunk.samples.len() / CH) as i64;
    if frames == 0 {
        return;
    }

    let estimated = frame_of(origin, chunk.captured_at);
    let start = match source.next_frame {
        Some(expected) if (estimated - expected).abs() <= SNAP_FRAMES => expected,
        _ => estimated,
    };
    source.next_frame = Some(start + frames);

    // Silêncio não soma nada (e deixa o trecho vazio, se ninguém mais tocou)
    if !chunk.silent {
        mix.add(start, &chunk.samples);
    }
}

/// Confere quais apps devem ser capturados agora e abre/fecha as capturas.
fn refresh(sources: &mut HashMap<u32, Source>, failed: &mut HashSet<u32>) {
    let wanted = wanted_pids();

    let gone: Vec<u32> = sources
        .keys()
        .filter(|pid| !wanted.contains(pid))
        .copied()
        .collect();
    for pid in gone {
        if let Some(source) = sources.remove(&pid) {
            let _ = source.capture.stop();
        }
    }

    // Um processo que falhou não é tentado de novo (quando ele fechar, sai daqui também)
    failed.retain(|pid| wanted.contains(pid));

    for pid in wanted {
        if sources.len() >= MAX_SOURCES {
            break;
        }
        if sources.contains_key(&pid) || failed.contains(&pid) {
            continue;
        }
        match AudioCapture::start(AudioSource::ProcessTree(pid)) {
            Ok(capture) => {
                sources.insert(
                    pid,
                    Source {
                        capture,
                        next_frame: None,
                    },
                );
            }
            Err(_) => {
                failed.insert(pid);
            }
        }
    }
}

/// Os processos que devem ser capturados agora (veja as regras no topo do arquivo).
fn wanted_pids() -> HashSet<u32> {
    let excluded: HashSet<String> = audio_sessions::excluded_apps().into_iter().collect();
    let processes = process_table();
    let own_pid = std::process::id();

    // Bloqueados: os apps da lista e o próprio LANShare
    let blocked: HashSet<u32> = processes
        .iter()
        .filter(|(pid, (_, exe))| **pid == own_pid || excluded.contains(exe))
        .map(|(pid, _)| *pid)
        .collect();

    // Quem tem um bloqueado na árvore abaixo dele não pode ser capturado
    let mut tainted: HashSet<u32> = HashSet::new();
    for pid in &blocked {
        tainted.extend(ancestors(*pid, &processes));
    }

    let Ok(apps) = (unsafe { audio_sessions::enumerate() }) else {
        return HashSet::new();
    };

    let candidates: HashSet<u32> = apps
        .iter()
        .filter(|app| !excluded.contains(&app.exe))
        .flat_map(|app| app.pids.iter().copied())
        .filter(|pid| !blocked.contains(pid) && !tainted.contains(pid))
        .filter(|pid| !ancestors(*pid, &processes).iter().any(|a| blocked.contains(a)))
        .collect();

    // Pai e filho com som: fica só o pai (a captura dele já inclui o filho)
    candidates
        .iter()
        .copied()
        .filter(|pid| !ancestors(*pid, &processes).iter().any(|a| candidates.contains(a)))
        .collect()
}

/// Todos os processos abertos: pid -> (pid do pai, .exe em minúsculas).
fn process_table() -> HashMap<u32, (u32, String)> {
    let mut table = HashMap::new();

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return table;
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                table.insert(entry.th32ProcessID, (entry.th32ParentProcessID, exe));

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    table
}

/// Pai, avô, bisavô... de um processo (sem ele mesmo).
fn ancestors(pid: u32, processes: &HashMap<u32, (u32, String)>) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut current = pid;

    for _ in 0..MAX_DEPTH {
        let Some((parent, _)) = processes.get(&current) else { break };
        let parent = *parent;
        if parent == 0 || parent == pid || chain.contains(&parent) {
            break;
        }
        chain.push(parent);
        current = parent;
    }

    chain
}

/// Quantos quadros (48 kHz) se passaram de `origin` até `at`.
fn frame_of(origin: Instant, at: Instant) -> i64 {
    let elapsed = at.saturating_duration_since(origin);
    (elapsed.as_micros() as i64) * SAMPLE_RATE as i64 / 1_000_000
}

fn frames_to_duration(frames: i64) -> Duration {
    Duration::from_micros((frames.max(0) as u64) * 1_000_000 / SAMPLE_RATE as u64)
}

/// A linha do tempo do mixer: soma o som de todos os apps no lugar certo.
#[derive(Default)]
struct MixBuffer {
    /// Quadro em que o buffer começa (tudo antes disso já saiu).
    start: i64,
    /// Soma das amostras (intercaladas), em 32 bits para não estourar no meio da soma.
    sums: VecDeque<i32>,
    /// Algum app tocou neste quadro?
    filled: VecDeque<bool>,
}

impl MixBuffer {
    /// Soma um bloco que começa no quadro `at`. O que chegou atrasado demais é descartado.
    fn add(&mut self, at: i64, samples: &[i16]) {
        let frames = samples.len() / CH;
        let mut skip = 0usize;
        if at < self.start {
            skip = (self.start - at) as usize;
            if skip >= frames {
                return;
            }
        }

        let offset = (at + skip as i64 - self.start) as usize;
        let end = offset + (frames - skip);
        if end > MAX_AHEAD_FRAMES {
            return;
        }

        if self.filled.len() < end {
            self.filled.resize(end, false);
            self.sums.resize(end * CH, 0);
        }

        for frame in 0..frames - skip {
            self.filled[offset + frame] = true;
            for channel in 0..CH {
                self.sums[(offset + frame) * CH + channel] += samples[(skip + frame) * CH + channel] as i32;
            }
        }
    }

    /// Tira tudo até o quadro `until` (sem ele). Devolve (quadro inicial, amostras) se alguém
    /// tocou nesse trecho; trecho todo em silêncio = None (vira um "buraco", como numa captura só).
    fn take_until(&mut self, until: i64) -> Option<(i64, Vec<i16>)> {
        if until <= self.start {
            return None;
        }

        let frames = (until - self.start) as usize;
        let available = self.filled.len().min(frames);
        let start = self.start;
        let any = self.filled.iter().take(available).any(|f| *f);

        let samples = if any {
            (0..frames * CH)
                .map(|i| {
                    let sum = self.sums.get(i).copied().unwrap_or(0);
                    sum.clamp(i16::MIN as i32, i16::MAX as i32) as i16
                })
                .collect()
        } else {
            Vec::new()
        };

        self.sums.drain(..available * CH);
        self.filled.drain(..available);
        self.start = until;

        any.then_some((start, samples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistura_dois_apps_no_mesmo_trecho() {
        let mut mix = MixBuffer::default();
        mix.add(0, &[100, 100, 100, 100]); // 2 quadros
        mix.add(1, &[50, 50, 50, 50]); // 2 quadros, começando no 2º

        let (start, samples) = mix.take_until(3).unwrap();
        assert_eq!(start, 0);
        assert_eq!(samples, vec![100, 100, 150, 150, 50, 50]);
    }

    #[test]
    fn soma_nao_estoura() {
        let mut mix = MixBuffer::default();
        mix.add(0, &[30_000, -30_000]);
        mix.add(0, &[30_000, -30_000]);
        let (_, samples) = mix.take_until(1).unwrap();
        assert_eq!(samples, vec![i16::MAX, i16::MIN]);
    }

    #[test]
    fn trecho_em_silencio_nao_sai_e_atrasado_e_descartado() {
        let mut mix = MixBuffer::default();
        assert!(mix.take_until(10).is_none());

        mix.add(5, &[1, 1]); // antes do início atual (10): descartado
        assert!(mix.take_until(20).is_none());

        mix.add(20, &[7, 7]);
        let (start, samples) = mix.take_until(21).unwrap();
        assert_eq!(start, 20);
        assert_eq!(samples, vec![7, 7]);
    }

    #[test]
    fn arvore_de_processos() {
        let mut table = HashMap::new();
        table.insert(1, (0, "explorer.exe".to_string()));
        table.insert(2, (1, "discord.exe".to_string()));
        table.insert(3, (2, "discord.exe".to_string()));
        assert_eq!(ancestors(3, &table), vec![2, 1]);
        assert!(ancestors(1, &table).is_empty());
    }
}
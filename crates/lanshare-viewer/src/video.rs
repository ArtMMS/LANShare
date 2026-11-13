//! Vídeo de quem assiste: decodifica o H.264 numa thread própria
//! e guarda a imagem mais recente para a interface buscar.
//!
//! Quando cada imagem aparece (Playout):
//! - com som tocando: quando o som chega no momento em que ela foi capturada (sync_clock.rs);
//! - sem som: numa agenda própria, no horário da captura + um atraso fixo pequeno
//!   (absorve as variações da rede sem acelerar nem travar).
//! Se a rede entregar imagens atrasadas ou acumuladas, a agenda pula para a mais nova
//! em vez de mostrar todas aceleradas. Cada pulo conta como sinal de conexão instável
//! (vai no evento "sync_stats" como "unstable").

use crate::decoder::{Decoder, Picture};
use crate::sync_clock::SyncClock;
use crate::EventTx;
use lanshare_core::i18n::tr;
use lanshare_core::protocol::VideoFrame;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Quantos pacotes podem esperar na fila antes de a decodificação "ficar para trás".
const QUEUE_SIZE: usize = 16;

/// Com som: uma imagem pode aparecer até este tanto antes do som (abaixo de ~20 ms ninguém percebe).
const SYNC_LEAD_US: u64 = 10_000;

/// Sem som: cada imagem aparece no horário em que foi capturada + este atraso.
/// É a folga que absorve as pequenas variações da rede.
const PLAYOUT_DELAY: Duration = Duration::from_millis(100);

/// Sem som: uma imagem que chega mais atrasada que isso (em relação à agenda) faz a agenda
/// pular para ela. As mais velhas aparecem e somem na hora (pulo em vez de fast-forward).
/// Com som: uma imagem que chega mais atrasada que isso em relação ao som conta como instabilidade.
const LATE_LIMIT: Duration = Duration::from_millis(150);

/// Sem som: se a imagem mais nova estiver agendada para mais longe que isso, há imagens
/// acumuladas (a rede soltou tudo de uma vez): a agenda pula para a mais nova.
const MAX_AHEAD: Duration = Duration::from_millis(300);

/// Maior espera de uma imagem. Se passar disso, ela aparece mesmo assim
/// (melhor fora de sincronia do que travada).
const MAX_HOLD: Duration = Duration::from_millis(350);

/// Limite de imagens esperando a hora (cada uma ocupa ~3 MB em 1080p).
const MAX_HELD: usize = 12;

/// De quanto em quanto tempo a thread confere se já é hora de mostrar a próxima imagem.
const RELEASE_POLL: Duration = Duration::from_millis(4);

/// Sem nada esperando, a thread só acorda quando chega pacote (ou para conferir o fim).
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// De quanto em quanto tempo o motor avisa a diferença entre imagem e som (e a instabilidade).
const SYNC_REPORT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
struct SlotData {
    /// Muda a cada conexão: imagens de uma conexão antiga são ignoradas.
    generation: u64,
    width: usize,
    height: usize,
    /// Pixels 0x00RRGGBB (na memória: B, G, R, 0 = BGRA).
    pixels: Vec<u32>,
    seq: u64,
    read_seq: u64,
}

impl SlotData {
    fn reset(&mut self) {
        self.width = 0;
        self.height = 0;
        self.pixels.clear();
        self.seq = 0;
        self.read_seq = 0;
    }
}

/// Guarda a imagem mais recente do vídeo recebido.
#[derive(Clone, Default)]
pub struct FrameSlot {
    inner: Arc<Mutex<SlotData>>,
}

impl FrameSlot {
    fn lock(&self) -> MutexGuard<'_, SlotData> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Começa uma conexão nova: limpa a imagem e devolve o número da conexão.
    pub fn begin_session(&self) -> u64 {
        let mut data = self.lock();
        data.generation += 1;
        data.reset();
        data.generation
    }

    /// Fim da conexão: limpa a imagem e invalida quem ainda estiver escrevendo.
    pub fn end_session(&self) {
        let mut data = self.lock();
        data.generation += 1;
        data.reset();
    }

    /// Guarda uma imagem nova (troca os buffers, sem copiar).
    fn publish(&self, generation: u64, width: usize, height: usize, pixels: &mut Vec<u32>) {
        let mut data = self.lock();
        if data.generation != generation {
            return;
        }
        std::mem::swap(&mut data.pixels, pixels);
        data.width = width;
        data.height = height;
        data.seq += 1;
    }

    /// Copia a imagem (BGRA) para `out`, só se for nova desde a última leitura.
    pub fn read_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        let mut data = self.lock();
        if data.seq == data.read_seq || data.pixels.is_empty() {
            return None;
        }

        let pixel_count = data.width * data.height;
        let needed = pixel_count * 4;
        if needed == 0 || data.pixels.len() < pixel_count || out.len() < needed {
            return None;
        }

        // SAFETY: `pixels` tem pelo menos pixel_count u32 (= needed bytes) e u8 não exige alinhamento
        let bytes = unsafe { std::slice::from_raw_parts(data.pixels.as_ptr() as *const u8, needed) };
        out[..needed].copy_from_slice(bytes);

        data.read_seq = data.seq;
        Some((data.width as u32, data.height as u32))
    }
}

/// A thread que decodifica o vídeo. Ao ser descartada, a thread termina sozinha.
pub struct VideoPipeline {
    tx: SyncSender<VideoFrame>,
    /// Ligado quando um pacote foi descartado (fila cheia).
    gap: Arc<AtomicBool>,
}

impl VideoPipeline {
    /// `clock` é o relógio do som desta transmissão (o mesmo do AudioPlayer).
    pub fn start(slot: FrameSlot, generation: u64, events: EventTx, clock: SyncClock) -> Self {
        let (tx, rx) = sync_channel(QUEUE_SIZE);
        let gap = Arc::new(AtomicBool::new(false));
        let thread_gap = gap.clone();

        // O decodificador precisa ser criado e usado na mesma thread
        let _ = std::thread::Builder::new()
            .name("lanshare-decoder".to_string())
            .spawn(move || run(rx, thread_gap, slot, generation, events, clock));

        Self { tx, gap }
    }

    /// Entrega um pacote. Nunca trava a conexão: se a fila estiver cheia, descarta.
    pub fn push(&self, frame: VideoFrame) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(frame) {
            self.gap.store(true, Ordering::Relaxed);
        }
    }
}

/// Decodificador + regras de quando decodificar.
struct Pipeline {
    decoder: Decoder,
    gap: Arc<AtomicBool>,
    /// true = descartando pacotes até chegar um keyframe (nada decodifica sem ele).
    waiting_keyframe: bool,
}

impl Pipeline {
    /// Decodifica um pacote e devolve a imagem mais recente que saiu dele (se saiu alguma),
    /// com o momento em que ela foi capturada em quem transmite.
    fn process(&mut self, frame: VideoFrame) -> Option<Held> {
        // Perdemos pacotes: os próximos dependem deles, então espera o próximo keyframe
        if self.gap.swap(false, Ordering::Relaxed) {
            self.wait_for_keyframe();
        }

        if self.waiting_keyframe {
            if !frame.keyframe {
                return None;
            }
            self.waiting_keyframe = false;
        }

        match self.decoder.decode(&frame.data, frame.timestamp_us) {
            Ok(mut pictures) => pictures.pop().map(|picture| Held {
                timestamp_us: frame.timestamp_us,
                since: Instant::now(),
                picture,
            }),
            Err(erro) => {
                println!("[viewer] Erro ao decodificar: {erro}");
                self.wait_for_keyframe();
                None
            }
        }
    }

    fn wait_for_keyframe(&mut self) {
        self.waiting_keyframe = true;
        self.decoder.flush();
    }
}

/// Uma imagem decodificada esperando a hora de aparecer.
struct Held {
    /// Quando ela foi capturada em quem transmite (µs desde o início da transmissão).
    timestamp_us: u64,
    /// Desde quando ela está esperando.
    since: Instant,
    picture: Picture,
}

// ───────────────────────────── Agenda de exibição ─────────────────────────────

/// "A imagem capturada em `stream_us` aparece em `at`" (as outras, na mesma proporção).
#[derive(Debug, Clone, Copy)]
struct Schedule {
    stream_us: u64,
    at: Instant,
}

impl Schedule {
    /// Quando a imagem capturada em `timestamp_us` deve aparecer.
    fn due_at(&self, timestamp_us: u64) -> Instant {
        if timestamp_us >= self.stream_us {
            self.at + Duration::from_micros(timestamp_us - self.stream_us)
        } else {
            let earlier = Duration::from_micros(self.stream_us - timestamp_us);
            self.at.checked_sub(earlier).unwrap_or(self.at)
        }
    }
}

/// Decide a hora de cada imagem: seguindo o som (quando há som tocando) ou numa agenda
/// própria (sem som). Não guarda imagens: só horários, por isso dá para testar sozinha.
#[derive(Default)]
struct Playout {
    schedule: Option<Schedule>,
    /// true = há som tocando: a imagem segue o som (e a agenda só acompanha).
    following_audio: bool,
}

impl Playout {
    /// Chegou uma imagem capturada em `timestamp_us`. Sem som, ajusta a agenda se preciso.
    /// Devolve true se a agenda teve que pular (imagem atrasada ou acumulada demais).
    fn on_frame(&mut self, timestamp_us: u64, now: Instant) -> bool {
        if self.following_audio {
            return false;
        }

        let (restart, jumped) = match self.schedule {
            None => (true, false),
            Some(schedule) => {
                let due = schedule.due_at(timestamp_us);
                // Chegou atrasada demais (a rede segurou) ou adiantada demais (a rede soltou
                // tudo de uma vez): esta imagem passa a ser o "agora", com a folga normal
                let jump = now > due + LATE_LIMIT || due > now + MAX_AHEAD;
                (jump, jump)
            }
        };

        if restart {
            self.schedule = Some(Schedule {
                stream_us: timestamp_us,
                at: now + PLAYOUT_DELAY,
            });
        }
        jumped
    }

    /// Antes de soltar as imagens: há som tocando? Se sim, a agenda acompanha o som,
    /// para o vídeo continuar no mesmo ritmo (sem pulo) se o som parar.
    fn follow_audio(&mut self, audio_us: Option<u64>, now: Instant) {
        self.following_audio = audio_us.is_some();
        if let Some(audio) = audio_us {
            self.schedule = Some(Schedule {
                stream_us: audio,
                at: now,
            });
        }
    }

    /// A imagem capturada em `timestamp_us` já pode aparecer?
    fn is_due(&self, timestamp_us: u64, audio_us: Option<u64>, now: Instant) -> bool {
        match (audio_us, self.schedule) {
            (Some(audio), _) => timestamp_us <= audio + SYNC_LEAD_US,
            (None, Some(schedule)) => schedule.due_at(timestamp_us) <= now,
            (None, None) => true,
        }
    }
}

/// Imagens decodificadas esperando a hora delas.
#[derive(Default)]
struct SyncQueue {
    held: VecDeque<Held>,
    playout: Playout,
    /// Quantas vezes a imagem chegou atrasada ou acumulada desde a última pergunta.
    jumps: u32,
}

impl SyncQueue {
    fn push(&mut self, held: Held, clock: &SyncClock) {
        if self.playout.on_frame(held.timestamp_us, Instant::now()) {
            self.jumps += 1;
        } else if self.playout.following_audio {
            // Com som: a imagem chegou muito depois do som que já está tocando
            let late_us = LATE_LIMIT.as_micros() as u64;
            if clock.now_us().is_some_and(|audio| held.timestamp_us + late_us < audio) {
                self.jumps += 1;
            }
        }
        self.held.push_back(held);
    }

    /// A imagem que deve aparecer agora (a mais nova que já está na hora).
    /// As mais velhas que também já estavam na hora são puladas.
    /// Devolve também a diferença entre a imagem e o som, quando há som.
    fn release(&mut self, clock: &SyncClock) -> Option<(Picture, Option<i64>)> {
        let now = Instant::now();
        let audio_us = clock.now_us();
        self.playout.follow_audio(audio_us, now);

        let mut chosen = None;

        while let Some(front) = self.held.front() {
            let due = self.playout.is_due(front.timestamp_us, audio_us, now)
                || front.since.elapsed() >= MAX_HOLD
                || self.held.len() > MAX_HELD;

            if !due {
                break;
            }

            let Some(held) = self.held.pop_front() else { break };
            // Positivo = a imagem apareceu antes do som; negativo = depois
            let offset = audio_us.map(|audio| held.timestamp_us as i64 - audio as i64);
            chosen = Some((held.picture, offset));
        }

        chosen
    }

    /// Quantos pulos houve desde a última pergunta (lê e zera).
    fn take_jumps(&mut self) -> u32 {
        std::mem::take(&mut self.jumps)
    }

    fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// Média da diferença entre imagem e som, para o evento "sync_stats"
/// (null quando nenhuma imagem do período teve som para acompanhar),
/// e se a conexão esteve instável no período (houve algum pulo).
struct SyncStats {
    since: Instant,
    total_us: i64,
    count: i64,
    jumps: u32,
}

impl SyncStats {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            total_us: 0,
            count: 0,
            jumps: 0,
        }
    }

    fn record(&mut self, offset_us: Option<i64>, jumps: u32, events: &EventTx) {
        if let Some(offset) = offset_us {
            self.total_us += offset;
            self.count += 1;
        }
        self.jumps += jumps;

        if self.since.elapsed() >= SYNC_REPORT_INTERVAL {
            let offset_ms =
                (self.count > 0).then(|| self.total_us as f64 / self.count as f64 / 1000.0);
            let _ = events.send(json!({
                "type": "sync_stats",
                "offset_ms": offset_ms,
                "unstable": self.jumps > 0,
            }));
            *self = Self::new();
        }
    }
}

fn run(
    rx: Receiver<VideoFrame>,
    gap: Arc<AtomicBool>,
    slot: FrameSlot,
    generation: u64,
    events: EventTx,
    clock: SyncClock,
) {
    let decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        Err(erro) => {
            let _ = events.send(json!({
                "type": "video_error",
                "error": format!(
                    "{}: {erro}",
                    tr(
                        "Não foi possível iniciar o decodificador de vídeo",
                        "Could not start the video decoder",
                        "No se pudo iniciar el decodificador de vídeo"
                    )
                ),
            }));
            return;
        }
    };

    let mut pipeline = Pipeline {
        decoder,
        gap,
        waiting_keyframe: true,
    };

    let mut queue = SyncQueue::default();
    let mut stats = SyncStats::new();
    let mut rgb: Vec<u32> = Vec::new();
    let mut size = (0usize, 0usize);

    loop {
        // Com imagens esperando, acorda logo para conferir a hora delas
        let wait = if queue.is_empty() { IDLE_WAIT } else { RELEASE_POLL };

        match rx.recv_timeout(wait) {
            Ok(frame) => {
                if let Some(held) = pipeline.process(frame) {
                    queue.push(held, &clock);
                }
                // Decodifica tudo o que já chegou
                while let Ok(frame) = rx.try_recv() {
                    if let Some(held) = pipeline.process(frame) {
                        queue.push(held, &clock);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // A conexão acabou e o VideoPipeline foi descartado
            Err(RecvTimeoutError::Disconnected) => return,
        }

        let Some((picture, offset)) = queue.release(&clock) else {
            continue;
        };
        stats.record(offset, queue.take_jumps(), &events);

        picture.to_rgb(&mut rgb);

        let new_size = (picture.width, picture.height);
        if new_size != size {
            size = new_size;
            let _ = events.send(json!({
                "type": "video_started",
                "width": new_size.0,
                "height": new_size.1,
            }));
        }

        slot.publish(generation, new_size.0, new_size.1, &mut rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_US: u64 = 33_333; // 30 FPS

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn rede_normal_cada_imagem_aparece_no_ritmo_da_captura() {
        let start = Instant::now();
        let mut playout = Playout::default();

        // A primeira imagem chega e é agendada para daqui a PLAYOUT_DELAY (não é um pulo)
        assert!(!playout.on_frame(0, start));
        assert!(!playout.is_due(0, None, start + ms(50)));
        assert!(playout.is_due(0, None, start + PLAYOUT_DELAY));

        // A segunda chega 33 ms depois e aparece 33 ms depois da primeira
        assert!(!playout.on_frame(FRAME_US, start + ms(33)));
        assert!(!playout.is_due(FRAME_US, None, start + PLAYOUT_DELAY + ms(20)));
        assert!(playout.is_due(FRAME_US, None, start + PLAYOUT_DELAY + ms(34)));
    }

    #[test]
    fn imagem_muito_atrasada_faz_a_agenda_pular_para_ela() {
        let start = Instant::now();
        let mut playout = Playout::default();
        playout.on_frame(0, start);

        // A imagem 10 (capturada em ~333 ms) tinha hora marcada para ~433 ms,
        // mas a rede segurou e ela só chegou em 1100 ms (~667 ms atrasada): é um pulo
        let late_arrival = start + ms(1_100);
        assert!(playout.on_frame(10 * FRAME_US, late_arrival));

        // A agenda recomeçou nela: as imagens mais velhas já passaram da hora (somem na hora)...
        assert!(playout.is_due(5 * FRAME_US, None, late_arrival));
        // ...e ela aparece com a folga normal (não acelera)
        assert!(!playout.is_due(10 * FRAME_US, None, late_arrival));
        assert!(playout.is_due(10 * FRAME_US, None, late_arrival + PLAYOUT_DELAY));
    }

    #[test]
    fn imagens_acumuladas_pulam_para_a_mais_nova() {
        let start = Instant::now();
        let mut playout = Playout::default();
        playout.on_frame(0, start);

        // A rede soltou 1 s de imagens de uma vez: a mais nova ficaria a ~1 s no futuro
        let burst = start + ms(10);
        assert!(playout.on_frame(30 * FRAME_US, burst));

        // A agenda pulou para ela: aparece com a folga normal, não daqui a 1 s
        assert!(playout.is_due(30 * FRAME_US, None, burst + PLAYOUT_DELAY));
        assert!(playout.is_due(15 * FRAME_US, None, burst));
    }

    #[test]
    fn com_som_segue_o_som_e_sem_som_continua_no_mesmo_ritmo() {
        let start = Instant::now();
        let mut playout = Playout::default();

        // Som tocando no momento 1 s da transmissão
        playout.follow_audio(Some(1_000_000), start);
        assert!(playout.is_due(1_000_000, Some(1_000_000), start));
        assert!(!playout.is_due(1_100_000, Some(1_000_000), start));

        // Com som, a imagem que chega não mexe na agenda (nem conta como pulo aqui)
        assert!(!playout.on_frame(1_100_000, start));

        // O som parou: a agenda continua de onde o som estava, sem pulo
        playout.follow_audio(None, start + ms(50));
        assert!(!playout.is_due(1_100_000, None, start + ms(50)));
        assert!(playout.is_due(1_100_000, None, start + ms(100)));
    }
}
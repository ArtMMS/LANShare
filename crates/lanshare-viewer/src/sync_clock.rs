//! Relógio da sincronia: o som manda, a imagem segue.
//!
//! A thread do som (audio_player.rs) anota qual momento da transmissão está saindo
//! nos alto-falantes agora. A thread do vídeo (video.rs) só mostra cada imagem quando o som
//! chega no momento em que ela foi capturada no Host. Os dois horários vêm do mesmo relógio
//! do Host (microssegundos desde o início da transmissão).
//!
//! Sem som tocando (transmissão sem áudio, silêncio, saída com problema), o relógio fica
//! "parado" e a imagem é mostrada assim que fica pronta, como antes.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Uma leitura do relógio do som vale por este tempo. Depois disso, o som é considerado parado.
const STALE_AFTER: Duration = Duration::from_millis(150);

#[derive(Clone, Copy)]
struct Reading {
    /// Momento da transmissão (µs, relógio do Host) que estava saindo no alto-falante...
    audible_us: u64,
    /// ...neste instante.
    at: Instant,
}

/// Clonar NÃO copia: todos os clones veem o mesmo relógio. Um relógio novo a cada transmissão.
#[derive(Clone, Default)]
pub struct SyncClock {
    inner: Arc<Mutex<Option<Reading>>>,
}

impl SyncClock {
    /// Chamado pelo som: `audible_us` está saindo no alto-falante agora.
    pub fn update(&self, audible_us: u64) {
        *self.lock() = Some(Reading {
            audible_us,
            at: Instant::now(),
        });
    }

    /// Chamado pelo som quando ele para (faltou som, saída fechou).
    pub fn pause(&self) {
        *self.lock() = None;
    }

    /// Momento da transmissão que está saindo no alto-falante agora.
    /// None = não há som tocando (a imagem não deve esperar).
    pub fn now_us(&self) -> Option<u64> {
        let reading = (*self.lock())?;
        let elapsed = reading.at.elapsed();
        if elapsed > STALE_AFTER {
            return None;
        }
        Some(reading.audible_us + elapsed.as_micros() as u64)
    }

    fn lock(&self) -> MutexGuard<'_, Option<Reading>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

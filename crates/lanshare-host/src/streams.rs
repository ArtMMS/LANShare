//! Quem está transmitindo na sala e onde assistir (a coordenação da v2.0).
//! O Host guarda só o endereço: o vídeo vai direto de quem transmite para quem assiste.

use lanshare_core::protocol::Message;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Onde assistir uma transmissão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSpot {
    /// IP de quem transmite, como o Host o enxerga. None = o próprio Host.
    pub ip: Option<String>,
    /// Porta do StreamServer de quem transmite.
    pub port: u16,
}

impl StreamSpot {
    /// O aviso "fulano está transmitindo" que vai para a sala.
    pub fn message(&self, user_id: u64) -> Message {
        Message::UserStreaming {
            user_id,
            ip: self.ip.clone(),
            port: self.port,
        }
    }
}

/// O catálogo de transmissões da sala (id de quem transmite -> onde assistir).
/// Clonar NÃO copia: todos os clones veem o mesmo catálogo.
#[derive(Clone, Default)]
pub struct StreamDirectory {
    inner: Arc<Mutex<HashMap<u64, StreamSpot>>>,
}

impl StreamDirectory {
    pub fn set(&self, id: u64, spot: StreamSpot) {
        self.lock().insert(id, spot);
    }

    /// Tira do catálogo. false = não estava transmitindo.
    pub fn remove(&self, id: u64) -> bool {
        self.lock().remove(&id).is_some()
    }

    pub fn get(&self, id: u64) -> Option<StreamSpot> {
        self.lock().get(&id).cloned()
    }

    pub fn contains(&self, id: u64) -> bool {
        self.lock().contains_key(&id)
    }

    /// Todas as transmissões, ordenadas pelo id de quem transmite.
    pub fn all(&self) -> Vec<(u64, StreamSpot)> {
        let mut list: Vec<(u64, StreamSpot)> =
            self.lock().iter().map(|(id, spot)| (*id, spot.clone())).collect();
        list.sort_by_key(|(id, _)| *id);
        list
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u64, StreamSpot>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}
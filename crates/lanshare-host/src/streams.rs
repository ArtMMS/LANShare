//! Quem está transmitindo na sala e onde assistir (a coordenação da v2.0).
//! O Host guarda só o endereço: o vídeo vai direto de quem transmite para quem assiste.
//! Cada pessoa pode ter uma transmissão de cada tipo ao mesmo tempo (tela e câmera).

use lanshare_core::protocol::{Message, StreamKind, StreamQuality};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Onde assistir uma transmissão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSpot {
    /// IP de quem transmite, como o Host o enxerga. None = o próprio Host.
    pub ip: Option<String>,
    /// Porta do StreamServer de quem transmite (a versão cheia).
    pub port: u16,
    /// Porta da versão pequena (720p, só a câmera tem). None = não tem.
    pub tile_port: Option<u16>,
}

impl StreamSpot {
    /// O aviso "fulano está transmitindo" que vai para a sala.
    pub fn message(&self, user_id: u64, kind: StreamKind) -> Message {
        Message::UserStreaming {
            user_id,
            kind,
            ip: self.ip.clone(),
            port: self.port,
            tile_port: self.tile_port,
        }
    }

    /// A porta de uma versão. None = essa versão não existe.
    pub fn port_for(&self, quality: StreamQuality) -> Option<u16> {
        match quality {
            StreamQuality::Full => Some(self.port),
            StreamQuality::Tile => self.tile_port,
        }
    }
}

/// O catálogo de transmissões da sala ((id de quem transmite, tipo) -> onde assistir).
/// Clonar NÃO copia: todos os clones veem o mesmo catálogo.
#[derive(Clone, Default)]
pub struct StreamDirectory {
    inner: Arc<Mutex<HashMap<(u64, StreamKind), StreamSpot>>>,
}

impl StreamDirectory {
    pub fn set(&self, id: u64, kind: StreamKind, spot: StreamSpot) {
        self.lock().insert((id, kind), spot);
    }

    /// Tira do catálogo. false = não estava transmitindo isso.
    pub fn remove(&self, id: u64, kind: StreamKind) -> bool {
        self.lock().remove(&(id, kind)).is_some()
    }

    pub fn get(&self, id: u64, kind: StreamKind) -> Option<StreamSpot> {
        self.lock().get(&(id, kind)).cloned()
    }

    /// O que essa pessoa está transmitindo agora (tela, câmera ou as duas).
    pub fn kinds_of(&self, id: u64) -> Vec<StreamKind> {
        let mut kinds: Vec<StreamKind> = self
            .lock()
            .keys()
            .filter(|(user, _)| *user == id)
            .map(|(_, kind)| *kind)
            .collect();
        kinds.sort();
        kinds
    }

    /// Todas as transmissões, ordenadas pelo id de quem transmite (e pelo tipo).
    pub fn all(&self) -> Vec<(u64, StreamKind, StreamSpot)> {
        let mut list: Vec<(u64, StreamKind, StreamSpot)> = self
            .lock()
            .iter()
            .map(|((id, kind), spot)| (*id, *kind, spot.clone()))
            .collect();
        list.sort_by_key(|(id, kind, _)| (*id, *kind));
        list
    }

    /// Quem transmite alguma coisa (cada pessoa uma vez só).
    pub fn streamers(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.lock().keys().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<(u64, StreamKind), StreamSpot>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

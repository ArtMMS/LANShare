//! As câmeras da sala nos quadrados (v2.0): a versão pequena (720p) de TODA câmera ligada,
//! cada uma na sua conexão, ao mesmo tempo. Usado pelo Host e pelo Client.
//!
//! - `add`: alguém ligou a câmera -> pede o ingresso (pelo `TileRequester`);
//! - `granted`: chegou o ingresso -> conecta (a mesma `WatchSession` de assistir, sem som);
//! - `remove`: a câmera desligou (ou a pessoa saiu);
//! - `frame_into`: a imagem mais recente da câmera de alguém (para o quadrado dela).
//!
//! Se a conexão cair ou o pedido ficar sem resposta, pede de novo sozinho (a cada
//! `RETRY_AFTER`), enquanto a câmera estiver ligada. Não manda eventos para a interface:
//! o quadrado só mostra o avatar até a primeira imagem chegar.

use crate::audio_player::{AudioOutputSetting, AudioVolume};
use crate::session::{WatchSession, WatchTarget};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

/// Depois de quanto tempo sem imagem (pedido sem resposta ou conexão que caiu) pede de novo.
const RETRY_AFTER: Duration = Duration::from_secs(3);

/// Pede o ingresso da versão pequena da câmera de `user_id`.
/// - Client: manda o WatchRequest ao Host e devolve None (o ingresso chega depois, em `granted`);
/// - Host: ele mesmo cria o ingresso e devolve Some((endereço, ingresso)) para conectar já.
pub type TileRequester = Arc<dyn Fn(u64) -> Option<(SocketAddr, String)> + Send + Sync>;

/// A câmera de uma pessoa.
#[derive(Default)]
struct Tile {
    session: Option<WatchSession>,
    /// Quando pedi o ingresso pela última vez (para pedir de novo se não vier).
    requested_at: Option<Instant>,
}

impl Tile {
    /// Precisa de um ingresso novo? (Sem conexão ativa e sem pedido recente.)
    fn needs_request(&self) -> bool {
        let connected = self.session.as_ref().is_some_and(|s| !s.is_finished());
        !connected && self.requested_at.map_or(true, |t| t.elapsed() >= RETRY_AFTER)
    }
}

#[derive(Default)]
struct State {
    tiles: HashMap<u64, Tile>,
    requester: Option<TileRequester>,
    next_id: u64,
}

/// Clonar NÃO copia: todos os clones controlam as mesmas câmeras.
#[derive(Clone)]
pub struct TileWatcher {
    state: Arc<Mutex<State>>,
    runtime: Handle,
}

impl TileWatcher {
    pub fn new(runtime: Handle) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            runtime,
        }
    }

    /// Define quem pede os ingressos (o Host precisa do próprio motor pronto para isso).
    pub fn set_requester(&self, requester: TileRequester) {
        self.lock().requester = Some(requester);
    }

    /// `user_id` ligou a câmera: pede a versão pequena.
    pub fn add(&self, user_id: u64) {
        self.lock().tiles.entry(user_id).or_default();
        self.request_if_needed(user_id);
    }

    /// Chegou o ingresso da câmera de `user_id`: conecta. false = não estava esperando (ignorado).
    pub fn granted(&self, user_id: u64, addr: SocketAddr, ticket: String) -> bool {
        let mut state = self.lock();
        state.next_id += 1;
        let watch_id = state.next_id;

        let Some(tile) = state.tiles.get_mut(&user_id) else {
            return false;
        };
        if tile.session.as_ref().is_some_and(|s| !s.is_finished()) {
            return false; // já está conectado
        }

        // Os avisos da conexão não interessam à interface (ela só pega a imagem): vão para um
        // canal sem ninguém do outro lado
        let (events, _) = std::sync::mpsc::channel();
        tile.session = Some(WatchSession::start(
            &self.runtime,
            WatchTarget {
                watch_id,
                user_id,
                addr,
                ticket,
            },
            AudioOutputSetting::default(),
            AudioVolume::default(),
            events,
        ));
        true
    }

    /// O Host recusou: tenta de novo depois de `RETRY_AFTER` (se a câmera continuar ligada).
    pub fn denied(&self, user_id: u64) {
        if let Some(tile) = self.lock().tiles.get_mut(&user_id) {
            tile.requested_at = Some(Instant::now());
        }
    }

    /// `user_id` desligou a câmera (ou saiu da sala): fecha a conexão.
    pub fn remove(&self, user_id: u64) {
        let tile = self.lock().tiles.remove(&user_id);
        if let Some(session) = tile.and_then(|t| t.session) {
            session.end(true);
        }
    }

    /// Fecha todas as câmeras (ao sair da sala ou fechar o motor) e esquece quem pede os ingressos.
    pub fn clear(&self) {
        let tiles: Vec<Tile> = {
            let mut state = self.lock();
            state.requester = None;
            state.tiles.drain().map(|(_, tile)| tile).collect()
        };
        for session in tiles.into_iter().filter_map(|t| t.session) {
            session.end(false);
        }
    }

    /// Quem está com a câmera ligada (ids).
    pub fn users(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.lock().tiles.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Copia a imagem mais recente da câmera de `user_id` (BGRA) para `out`, só se for nova.
    /// Devolve (largura, altura). Se a câmera está sem conexão há um tempo, pede de novo.
    pub fn frame_into(&self, user_id: u64, out: &mut [u8]) -> Option<(u32, u32)> {
        let (frame, needs_request) = {
            let state = self.lock();
            let tile = state.tiles.get(&user_id)?;
            (
                tile.session.as_ref().map(|s| s.frame.clone()),
                tile.needs_request(),
            )
        };

        if needs_request {
            self.request_if_needed(user_id);
        }
        frame?.read_into(out)
    }

    /// Pede o ingresso, se a câmera ainda precisa (fora do lock: o Host conecta na hora).
    fn request_if_needed(&self, user_id: u64) {
        let requester = {
            let mut state = self.lock();
            let Some(requester) = state.requester.clone() else {
                return;
            };
            let Some(tile) = state.tiles.get_mut(&user_id) else {
                return;
            };
            if !tile.needs_request() {
                return;
            }
            tile.requested_at = Some(Instant::now());
            tile.session = None; // a conexão antiga (se havia) já acabou
            requester
        };

        if let Some((addr, ticket)) = requester(user_id) {
            self.granted(user_id, addr, ticket);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

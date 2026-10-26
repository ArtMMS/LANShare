//! Quem assiste: no máximo UMA transmissão por vez (v2.0). Usado pelo Host e pelo Client.
//!
//! - Client: `request` (pediu ao Host) e depois `granted` (chegou o WatchGranted);
//! - Host: `watch_now` (ele mesmo cria o ingresso e conecta direto).
//! A imagem da transmissão assistida sai por `frame_into`.

use crate::audio_player::{AudioOutputSetting, AudioVolume};
use crate::session::{WatchSession, WatchTarget};
use crate::EventTx;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::runtime::Handle;

#[derive(Default)]
struct State {
    /// Pedi ao Host para assistir este usuário e ainda espero a resposta.
    wanted: Option<u64>,
    session: Option<WatchSession>,
    next_id: u64,
}

/// Clonar NÃO copia: todos os clones controlam o mesmo "assistir".
#[derive(Clone)]
pub struct Watcher {
    state: Arc<Mutex<State>>,
    runtime: Handle,
    events: EventTx,
    output: AudioOutputSetting,
    volume: AudioVolume,
}

impl Watcher {
    /// `output` e `volume` são os mesmos das Configurações: mudar vale na hora, mesmo assistindo.
    pub fn new(runtime: Handle, events: EventTx, output: AudioOutputSetting, volume: AudioVolume) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            runtime,
            events,
            output,
            volume,
        }
    }

    /// Client: pediu ao Host para assistir `user_id`. Para o que estava assistindo.
    pub fn request(&self, user_id: u64) {
        let mut state = self.lock();
        state.wanted = Some(user_id);
        if let Some(old) = state.session.take() {
            old.end(false);
        }
    }

    /// Client: chegou o WatchGranted. false = não foi pedido (ou já desisti): ignorado.
    pub fn granted(&self, user_id: u64, addr: SocketAddr, ticket: String) -> bool {
        let mut state = self.lock();
        if state.wanted != Some(user_id) {
            return false;
        }
        state.wanted = None;
        self.start(&mut state, user_id, addr, ticket);
        true
    }

    /// Host: já tem o ingresso, conecta direto (para o que estava assistindo).
    pub fn watch_now(&self, user_id: u64, addr: SocketAddr, ticket: String) {
        let mut state = self.lock();
        state.wanted = None;
        if let Some(old) = state.session.take() {
            old.end(false);
        }
        self.start(&mut state, user_id, addr, ticket);
    }

    /// Client: o Host recusou. true = era o pedido em andamento (vale avisar a interface).
    pub fn denied(&self, user_id: u64) -> bool {
        let mut state = self.lock();
        if state.wanted == Some(user_id) {
            state.wanted = None;
            true
        } else {
            false
        }
    }

    /// A sala avisou que `user_id` parou de transmitir: se era quem eu assistia, encerra.
    pub fn stream_ended(&self, user_id: u64) {
        let mut state = self.lock();
        if state.wanted == Some(user_id) {
            state.wanted = None;
        }
        if state.session.as_ref().is_some_and(|s| s.user_id == user_id) {
            if let Some(session) = state.session.take() {
                session.end(true);
            }
        }
    }

    /// Para de assistir (e desiste de um pedido em andamento).
    pub fn stop(&self) {
        let mut state = self.lock();
        state.wanted = None;
        if let Some(session) = state.session.take() {
            session.end(false);
        }
    }

    /// Quem estou assistindo (ou pedi para assistir). None = ninguém.
    pub fn watching(&self) -> Option<u64> {
        let state = self.lock();
        state
            .session
            .as_ref()
            .filter(|s| !s.is_finished())
            .map(|s| s.user_id)
            .or(state.wanted)
    }

    /// Copia a imagem mais recente (BGRA) para `out`, só se for nova desde a última leitura.
    pub fn frame_into(&self, out: &mut [u8]) -> Option<(u32, u32)> {
        let frame = self.lock().session.as_ref()?.frame.clone();
        frame.read_into(out)
    }

    fn start(&self, state: &mut State, user_id: u64, addr: SocketAddr, ticket: String) {
        state.next_id += 1;
        let watch_id = state.next_id;

        let _ = self.events.send(json!({
            "type": "watch_connecting",
            "user_id": user_id,
            "watch_id": watch_id,
        }));

        state.session = Some(WatchSession::start(
            &self.runtime,
            WatchTarget {
                watch_id,
                user_id,
                addr,
                ticket,
            },
            self.output.clone(),
            self.volume.clone(),
            self.events.clone(),
        ));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanshare_core::protocol::new_watch_ticket;
    use lanshare_stream::{StreamEvents, StreamServer, StreamSink};
    use serde_json::Value;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::{Duration, Instant};

    /// Espera um evento de um tipo (os outros são pulados).
    fn wait_for(rx: &Receiver<Value>, kind: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("o evento {kind} nao chegou"));
            if event["type"] == kind {
                return event;
            }
        }
    }

    #[test]
    fn assiste_mesmo_com_o_ingresso_atrasado_e_percebe_o_fim() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = StreamServer::start(runtime.handle(), StreamEvents::new(|_| {})).unwrap();

        let (tx, rx) = channel();
        let watcher = Watcher::new(
            runtime.handle().clone(),
            tx,
            AudioOutputSetting::default(),
            AudioVolume::default(),
        );

        // Quem assiste chega antes do ingresso (a mesma corrida da rede de verdade)
        let ticket = new_watch_ticket();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port()));
        watcher.watch_now(5, addr, ticket.clone());
        std::thread::sleep(Duration::from_millis(150));
        assert!(server.allow_viewer(9, ticket));

        let started = wait_for(&rx, "watch_started");
        assert_eq!(started["user_id"], 5);
        assert_eq!(watcher.watching(), Some(5));
        assert!(server.has_viewers());

        // Quem transmite parou: quem assiste fica sabendo
        server.stop();
        let ended = wait_for(&rx, "watch_ended");
        assert_eq!(ended["stream_stopped"], true);

        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(watcher.watching(), None);
    }
}
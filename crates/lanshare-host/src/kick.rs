//! Expulsão: permite ao Host encerrar a conexão de UM Client.

use lanshare_core::protocol::Message;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

/// Tempo entre avisar o Client ("Kicked") e fechar a conexão, para o aviso chegar.
const KICK_GRACE: Duration = Duration::from_millis(300);

struct KickHandle {
    out_tx: UnboundedSender<Message>,
    stop_tx: oneshot::Sender<()>,
}

#[derive(Default)]
struct State {
    handles: HashMap<u64, KickHandle>,
    kicked: HashSet<u64>,
}

#[derive(Clone, Default)]
pub struct Kicker {
    state: Arc<Mutex<State>>,
}

impl Kicker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Chamado quando um Client entra: guarda como falar com ele e como parar a conexão.
    pub fn register(
        &self,
        id: u64,
        out_tx: UnboundedSender<Message>,
        stop_tx: oneshot::Sender<()>,
    ) {
        self.lock().handles.insert(id, KickHandle { out_tx, stop_tx });
    }

    /// Avisa o Client e, logo depois, encerra a conexão dele.
    /// Devolve false se não existe ou se já está saindo.
    pub fn kick(&self, id: u64, reason: &str) -> bool {
        let handle = {
            let mut state = self.lock();
            let Some(handle) = state.handles.remove(&id) else {
                return false;
            };
            state.kicked.insert(id);
            handle
        };

        let _ = handle.out_tx.send(Message::Kicked {
            reason: reason.to_string(),
        });

        tokio::spawn(async move {
            tokio::time::sleep(KICK_GRACE).await;
            let _ = handle.stop_tx.send(());
        });

        true
    }

    /// Chamado no fim da conexão: limpa o registro e diz se o Client foi expulso.
    pub fn finish(&self, id: u64) -> bool {
        let mut state = self.lock();
        state.handles.remove(&id);
        state.kicked.remove(&id)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}
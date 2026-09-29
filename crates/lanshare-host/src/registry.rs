//! Registro das conexões ativas do Host.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub type ClientId = u64;

/// Tamanho máximo do nome de um usuário (em caracteres).
const MAX_NAME_LEN: usize = 32;

/// Dados de um Client conectado.
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub id: ClientId,
    pub name: String,
    pub addr: SocketAddr,
    pub connected_at: Instant,
}

struct Inner {
    next_id: AtomicU64,           // contador de IDs, seguro entre tarefas
    max_clients: Option<usize>,   // None = sem limite
    clients: Mutex<HashMap<ClientId, ClientInfo>>,
}

/// Clonar um Registry NÃO copia a lista: todos os clones compartilham a mesma.
#[derive(Clone)]
pub struct Registry {
    inner: Arc<Inner>,
}

impl Registry {
    pub fn new(max_clients: Option<usize>) -> Self {
        Self {
            inner: Arc::new(Inner {
                next_id: AtomicU64::new(1),
                max_clients,
                clients: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Registra um Client (com nome limpo e único) e devolve os dados dele.
    /// Devolve None se a sala estiver cheia.
    pub fn try_add(&self, raw_name: &str, addr: SocketAddr) -> Option<ClientInfo> {
        let mut clients = self.inner.clients.lock().unwrap();

        if let Some(max) = self.inner.max_clients {
            if clients.len() >= max {
                return None;
            }
        }

        let name = unique_name(&sanitize_name(raw_name), &clients);
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let info = ClientInfo {
            id,
            name,
            addr,
            connected_at: Instant::now(),
        };
        clients.insert(id, info.clone());
        Some(info)
    }

    /// Tira um Client da lista.
    pub fn remove(&self, id: ClientId) -> Option<ClientInfo> {
        self.inner.clients.lock().unwrap().remove(&id)
    }

    /// Quantos Clients estão conectados agora.
    pub fn count(&self) -> usize {
        self.inner.clients.lock().unwrap().len()
    }

    /// Cópia da lista atual, ordenada por ID.
    pub fn list(&self) -> Vec<ClientInfo> {
        let mut list: Vec<ClientInfo> =
            self.inner.clients.lock().unwrap().values().cloned().collect();
        list.sort_by_key(|c| c.id);
        list
    }
}

/// Limpa um nome vindo da rede: sem caracteres de controle (ex.: quebra de linha),
/// no máximo MAX_NAME_LEN caracteres e sem espaços nas pontas.
fn sanitize_name(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect();

    let name = filtered.trim();
    if name.is_empty() {
        "Sem nome".to_string()
    } else {
        name.to_string()
    }
}

/// Se o nome já está em uso, acrescenta " (2)", " (3)"... (ignora maiúsculas).
fn unique_name(base: &str, clients: &HashMap<ClientId, ClientInfo>) -> String {
    let taken: HashSet<String> = clients.values().map(|c| c.name.to_lowercase()).collect();

    let mut candidate = base.to_string();
    let mut n = 2;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("{base} ({n})");
        n += 1;
    }
    candidate
}
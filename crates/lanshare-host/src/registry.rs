//! Registro das conexões ativas do Host.

use lanshare_core::protocol::{Message, UserInfo, VideoFrame};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

pub type ClientId = u64;

/// Tamanho máximo do nome de um usuário (em caracteres).
const MAX_NAME_LEN: usize = 32;

/// Quantos frames de vídeo podem esperar na fila de um Client.
/// Passou disso, ele está atrasado: descartamos frames dele (ver broadcast_video).
const MAX_QUEUED_VIDEO_FRAMES: usize = 8;

/// Dados de um Client conectado.
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub id: ClientId,
    pub name: String,
    pub addr: SocketAddr,
    pub connected_at: Instant,
}

/// Situação do vídeo para um Client.
struct VideoState {
    /// Quantos frames de vídeo estão esperando na fila deste Client.
    queued: Arc<AtomicUsize>,
    /// true = não manda nada até o próximo keyframe (acabou de entrar ou ficou para trás).
    waiting_keyframe: bool,
}

/// Um Client na lista: os dados dele + a caixa de saída para mandar mensagens.
struct Entry {
    info: ClientInfo,
    tx: UnboundedSender<Message>,
    video: VideoState,
}

struct Inner {
    next_id: AtomicU64,           // contador de IDs, seguro entre tarefas
    max_clients: Option<usize>,   // None = sem limite
    clients: Mutex<HashMap<ClientId, Entry>>,
    keyframe_requested: AtomicBool, // alguém precisa de um keyframe novo
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
                keyframe_requested: AtomicBool::new(false),
            }),
        }
    }

    /// Registra um Client (com nome limpo e único) e devolve os dados dele.
    /// `tx` é a caixa de saída da conexão dele. Devolve None se a sala estiver cheia.
    pub fn try_add(
        &self,
        raw_name: &str,
        addr: SocketAddr,
        tx: UnboundedSender<Message>,
    ) -> Option<ClientInfo> {
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
        clients.insert(
            id,
            Entry {
                info: info.clone(),
                tx,
                video: VideoState {
                    queued: Arc::new(AtomicUsize::new(0)),
                    waiting_keyframe: true,
                },
            },
        );

        // Quem entra só consegue assistir a partir de um keyframe
        self.inner.keyframe_requested.store(true, Ordering::Relaxed);

        Some(info)
    }

    /// Tira um Client da lista.
    pub fn remove(&self, id: ClientId) -> Option<ClientInfo> {
        self.inner.clients.lock().unwrap().remove(&id).map(|e| e.info)
    }

    /// Cópia da lista atual, ordenada por ID.
    pub fn list(&self) -> Vec<ClientInfo> {
        let mut list: Vec<ClientInfo> = self
            .inner
            .clients
            .lock()
            .unwrap()
            .values()
            .map(|e| e.info.clone())
            .collect();
        list.sort_by_key(|c| c.id);
        list
    }

    /// true se não há ninguém conectado.
    pub fn is_empty(&self) -> bool {
        self.inner.clients.lock().unwrap().is_empty()
    }

    /// A lista no formato que os Clients recebem (ID + nome).
    pub fn users(&self) -> Vec<UserInfo> {
        self.list()
            .into_iter()
            .map(|c| UserInfo {
                id: c.id,
                name: c.name,
            })
            .collect()
    }

    /// Envia uma mensagem a todos os Clients, menos o `except` (se houver).
    pub fn broadcast(&self, message: &Message, except: Option<ClientId>) {
        let clients = self.inner.clients.lock().unwrap();
        for (id, entry) in clients.iter() {
            if Some(*id) == except {
                continue;
            }
            // Erro = a conexão dele já terminou; não tem problema
            let _ = entry.tx.send(message.clone());
        }
    }

    /// Envia um pacote de vídeo a todos os Clients.
    ///
    /// - Quem acabou de entrar (ou ficou para trás) só recebe a partir de um keyframe.
    /// - Quem está com a fila cheia está atrasado: este frame é descartado para ele,
    ///   e pedimos um keyframe novo para ele voltar a acompanhar.
    pub fn broadcast_video(&self, keyframe: bool, timestamp_us: u64, data: Arc<Vec<u8>>) {
        let mut clients = self.inner.clients.lock().unwrap();

        for entry in clients.values_mut() {
            if entry.video.waiting_keyframe {
                if !keyframe {
                    continue;
                }
                entry.video.waiting_keyframe = false;
            }

            if entry.video.queued.load(Ordering::Relaxed) >= MAX_QUEUED_VIDEO_FRAMES {
                entry.video.waiting_keyframe = true;
                self.inner.keyframe_requested.store(true, Ordering::Relaxed);
                continue;
            }

            entry.video.queued.fetch_add(1, Ordering::Relaxed);
            let frame = VideoFrame::new(keyframe, timestamp_us, data.clone())
                .with_queue_counter(entry.video.queued.clone());

            // Erro = a conexão já terminou; o frame é descartado e o contador desce sozinho
            let _ = entry.tx.send(Message::Video(frame));
        }
    }

    /// Alguém precisa de um keyframe novo? (Lê e zera o pedido.)
    pub fn take_keyframe_request(&self) -> bool {
        self.inner.keyframe_requested.swap(false, Ordering::Relaxed)
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
fn unique_name(base: &str, clients: &HashMap<ClientId, Entry>) -> String {
    let taken: HashSet<String> = clients
        .values()
        .map(|e| e.info.name.to_lowercase())
        .collect();

    let mut candidate = base.to_string();
    let mut n = 2;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("{base} ({n})");
        n += 1;
    }
    candidate
}
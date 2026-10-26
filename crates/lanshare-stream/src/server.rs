//! Servidor de transmissão: quem transmite abre uma porta TCP e quem tem ingresso assiste por ela.
//!
//! 1. O Host avisa quem vai chegar: `allow_viewer(id, ingresso)`.
//! 2. O espectador conecta e manda WatchHello com o ingresso (vale uma vez, por WATCH_TICKET_TTL).
//! 3. Aceito: WatchAccepted, e depois Video/Audio pela mesma conexão (com Ping/Pong e Bye).
//!
//! É o StreamSink da transmissão: cada espectador tem a própria fila (quem atrasa perde vídeo,
//! nunca som) e só começa a receber num keyframe.
//!
//! Um servidor por transmissão. Sempre chame `stop()` ao terminar: é ele que fecha a porta.

use crate::events::StreamEvents;
use crate::sink::StreamSink;
use lanshare_core::protocol::{
    read_message, ticket_looks_valid, write_message, AudioFrame, Message, VideoFrame,
    PROTOCOL_VERSION, REJECT_INVALID_TICKET, REJECT_PROTOCOL_MISMATCH, WATCH_TICKET_TTL,
};
use lanshare_net::run_connection;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

/// Quantos frames de vídeo podem esperar na fila de um espectador.
/// Passou disso, ele está atrasado: descartamos frames dele até o próximo keyframe.
const MAX_QUEUED_VIDEO_FRAMES: usize = 8;

/// Tempo que o espectador tem para mandar o WatchHello depois de conectar.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Tempo para o StreamStopped chegar aos espectadores antes de fechar as conexões.
const STOP_GRACE: Duration = Duration::from_millis(300);

/// Um espectador conectado.
struct Viewer {
    /// Número desta conexão (se o mesmo espectador reconectar, a antiga não apaga a nova).
    connection: u64,
    tx: mpsc::UnboundedSender<Message>,
    stop: watch::Sender<bool>,
    /// Quantos frames de vídeo estão esperando na fila dele.
    queued: Arc<AtomicUsize>,
    /// true = não manda nada até o próximo keyframe (acabou de entrar ou ficou para trás).
    waiting_keyframe: bool,
}

struct Inner {
    runtime: Handle,
    events: StreamEvents,
    /// Ingresso -> (id do espectador, quando foi emitido).
    tickets: Mutex<HashMap<String, (u64, Instant)>>,
    viewers: Mutex<HashMap<u64, Viewer>>,
    next_connection: AtomicU64,
    keyframe_requested: AtomicBool,
    closed: AtomicBool,
    /// Para o laço que aceita conexões.
    shutdown: watch::Sender<bool>,
}

impl Inner {
    fn lock_tickets(&self) -> MutexGuard<'_, HashMap<String, (u64, Instant)>> {
        self.tickets.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_viewers(&self) -> MutexGuard<'_, HashMap<u64, Viewer>> {
        self.viewers.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Usa o ingresso (some da lista). Devolve o id do espectador se ele valia.
    fn redeem(&self, ticket: &str) -> Option<u64> {
        if self.closed.load(Ordering::SeqCst) || !ticket_looks_valid(ticket) {
            return None;
        }
        let (viewer_id, issued) = self.lock_tickets().remove(ticket)?;
        (issued.elapsed() < WATCH_TICKET_TTL).then_some(viewer_id)
    }

    /// Registra o espectador. Se ele já estava conectado, a conexão antiga é encerrada.
    fn add_viewer(&self, viewer_id: u64, tx: mpsc::UnboundedSender<Message>, stop: watch::Sender<bool>) -> u64 {
        let connection = self.next_connection.fetch_add(1, Ordering::Relaxed);
        let old = self.lock_viewers().insert(
            viewer_id,
            Viewer {
                connection,
                tx,
                stop,
                queued: Arc::new(AtomicUsize::new(0)),
                waiting_keyframe: true,
            },
        );
        if let Some(old) = old {
            let _ = old.stop.send(true);
        }

        // Quem entra só consegue assistir a partir de um keyframe
        self.keyframe_requested.store(true, Ordering::Relaxed);
        connection
    }

    /// Tira o espectador da lista, se ainda for esta conexão.
    fn remove_viewer(&self, viewer_id: u64, connection: u64) {
        let mut viewers = self.lock_viewers();
        if viewers.get(&viewer_id).is_some_and(|v| v.connection == connection) {
            viewers.remove(&viewer_id);
        }
    }
}

/// A porta de uma transmissão. Clonar NÃO cria outro servidor: todos os clones são o mesmo.
#[derive(Clone)]
pub struct StreamServer {
    inner: Arc<Inner>,
    port: u16,
}

impl StreamServer {
    /// Abre a porta (escolhida pelo Windows) e começa a aceitar espectadores.
    /// `runtime`: o runtime do tokio de quem transmite (Host ou Client).
    pub fn start(runtime: &Handle, events: StreamEvents) -> io::Result<Self> {
        let std_listener = std::net::TcpListener::bind(("0.0.0.0", 0))?;
        std_listener.set_nonblocking(true)?;
        let port = std_listener.local_addr()?.port();

        let _guard = runtime.enter();
        let listener = TcpListener::from_std(std_listener)?;

        let (shutdown, shutdown_rx) = watch::channel(false);
        let inner = Arc::new(Inner {
            runtime: runtime.clone(),
            events,
            tickets: Mutex::new(HashMap::new()),
            viewers: Mutex::new(HashMap::new()),
            next_connection: AtomicU64::new(1),
            keyframe_requested: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            shutdown,
        });

        runtime.spawn(accept_loop(listener, inner.clone(), shutdown_rx));
        inner.events.debug(format!("servidor de transmissao na porta {port}"));

        Ok(Self { inner, port })
    }

    /// Porta TCP onde os espectadores conectam (vai no StreamAnnounce).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Autoriza um espectador: ele vai chegar com este ingresso.
    /// false = ingresso com formato inválido ou servidor já parado.
    pub fn allow_viewer(&self, viewer_id: u64, ticket: String) -> bool {
        if self.inner.closed.load(Ordering::SeqCst) || !ticket_looks_valid(&ticket) {
            return false;
        }

        let mut tickets = self.inner.lock_tickets();
        tickets.retain(|_, (_, issued)| issued.elapsed() < WATCH_TICKET_TTL);
        tickets.insert(ticket, (viewer_id, Instant::now()));
        true
    }

    /// Derruba um espectador (saiu da sala ou foi expulso) e invalida os ingressos dele.
    pub fn revoke_viewer(&self, viewer_id: u64) {
        self.inner.lock_tickets().retain(|_, (id, _)| *id != viewer_id);
        if let Some(viewer) = self.inner.lock_viewers().remove(&viewer_id) {
            let _ = viewer.stop.send(true);
        }
    }

    /// Quem está assistindo agora (ids).
    pub fn viewers(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.inner.lock_viewers().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Para de aceitar espectadores, avisa quem assiste (StreamStopped) e fecha as conexões.
    pub fn stop(&self) {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.inner.shutdown.send(true);
        self.inner.lock_tickets().clear();

        let viewers: Vec<Viewer> = self.inner.lock_viewers().drain().map(|(_, v)| v).collect();
        for viewer in &viewers {
            let _ = viewer.tx.send(Message::StreamStopped);
        }

        // Dá tempo para o aviso chegar antes de fechar
        self.inner.runtime.spawn(async move {
            tokio::time::sleep(STOP_GRACE).await;
            for viewer in viewers {
                let _ = viewer.stop.send(true);
            }
        });
    }
}

impl StreamSink for StreamServer {
    fn has_viewers(&self) -> bool {
        !self.inner.lock_viewers().is_empty()
    }

    fn viewer_count(&self) -> usize {
        self.inner.lock_viewers().len()
    }

    fn take_keyframe_request(&self) -> bool {
        self.inner.keyframe_requested.swap(false, Ordering::Relaxed)
    }

    fn send_video(&self, keyframe: bool, timestamp_us: u64, data: Arc<Vec<u8>>) {
        let mut viewers = self.inner.lock_viewers();

        for viewer in viewers.values_mut() {
            if viewer.waiting_keyframe {
                if !keyframe {
                    continue;
                }
                viewer.waiting_keyframe = false;
            }

            // Fila cheia: ele está atrasado. Pula este frame e pede um keyframe para ele voltar
            if viewer.queued.load(Ordering::Relaxed) >= MAX_QUEUED_VIDEO_FRAMES {
                viewer.waiting_keyframe = true;
                self.inner.keyframe_requested.store(true, Ordering::Relaxed);
                continue;
            }

            viewer.queued.fetch_add(1, Ordering::Relaxed);
            let frame = VideoFrame::new(keyframe, timestamp_us, data.clone())
                .with_queue_counter(viewer.queued.clone());

            // Erro = a conexão já terminou; o frame é descartado e o contador desce sozinho
            let _ = viewer.tx.send(Message::Video(frame));
        }
    }

    fn send_audio(&self, timestamp_us: u64, data: Arc<Vec<u8>>) {
        // O som é pequeno e nunca é descartado: um buraco no som incomoda mais que um frame perdido
        for viewer in self.inner.lock_viewers().values() {
            let _ = viewer.tx.send(Message::Audio(AudioFrame {
                timestamp_us,
                data: data.clone(),
            }));
        }
    }
}

/// Aceita espectadores até o servidor parar.
async fn accept_loop(listener: TcpListener, inner: Arc<Inner>, mut shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, addr)) => {
                    tokio::spawn(handle_viewer(stream, addr, inner.clone()));
                }
                Err(erro) => {
                    inner.events.debug(format!("falha ao aceitar um espectador: {erro}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            _ = shutdown.changed() => break,
        }
    }
}

/// Cuida de UM espectador: confere o ingresso e mantém a conexão até ela acabar.
async fn handle_viewer(mut stream: TcpStream, addr: SocketAddr, inner: Arc<Inner>) {
    // Sem isso o TCP junta pacotes pequenos e atrasa o vídeo
    let _ = stream.set_nodelay(true);

    let Ok(Ok(Message::WatchHello { protocol_version, ticket })) =
        timeout(HANDSHAKE_TIMEOUT, read_message(&mut stream)).await
    else {
        return; // não mandou o WatchHello a tempo (ou mandou outra coisa)
    };

    if protocol_version != PROTOCOL_VERSION {
        let _ = refuse(&mut stream, "versao do protocolo incompativel", REJECT_PROTOCOL_MISMATCH).await;
        return;
    }

    let Some(viewer_id) = inner.redeem(&ticket) else {
        inner.events.debug(format!("ingresso invalido vindo de {addr}"));
        let _ = refuse(&mut stream, "ingresso invalido", REJECT_INVALID_TICKET).await;
        return;
    };

    // Registra antes de responder: o que chegar no meio já fica na fila, depois do WatchAccepted
    let (tx, rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = watch::channel(false);
    let connection = inner.add_viewer(viewer_id, tx, stop_tx);

    if write_message(&mut stream, &Message::WatchAccepted).await.is_err() {
        inner.remove_viewer(viewer_id, connection);
        return;
    }
    inner.events.debug(format!("espectador #{viewer_id} conectado ({addr})"));

    // O espectador só manda Ping/Pong/Bye: não há mensagens para tratar
    let reason = run_connection(stream, stop_rx, rx, |_| {}).await;

    inner.remove_viewer(viewer_id, connection);
    inner.events.debug(format!("espectador #{viewer_id} saiu: {reason}"));
}

async fn refuse(stream: &mut TcpStream, reason: &str, code: &str) -> io::Result<()> {
    write_message(
        stream,
        &Message::Rejected {
            reason: reason.to_string(),
            code: code.to_string(),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanshare_core::protocol::new_watch_ticket;

    fn new_server(runtime: &tokio::runtime::Runtime) -> StreamServer {
        StreamServer::start(runtime.handle(), StreamEvents::new(|_| {})).unwrap()
    }

    /// Conecta como espectador e devolve a resposta ao WatchHello.
    async fn connect(port: u16, ticket: &str) -> (TcpStream, Message) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let hello = Message::WatchHello {
            protocol_version: PROTOCOL_VERSION,
            ticket: ticket.to_string(),
        };
        write_message(&mut stream, &hello).await.unwrap();
        let reply = read_message(&mut stream).await.unwrap();
        (stream, reply)
    }

    /// Próxima mensagem que não seja Ping/Pong. None = a conexão acabou (Bye ou fechada).
    async fn next_message(stream: &mut TcpStream) -> Option<Message> {
        loop {
            match read_message(stream).await {
                Ok(Message::Ping { .. } | Message::Pong { .. }) => continue,
                Ok(Message::Bye) | Err(_) => return None,
                Ok(other) => return Some(other),
            }
        }
    }

    fn is_rejected_ticket(message: &Message) -> bool {
        matches!(message, Message::Rejected { code, .. } if code == REJECT_INVALID_TICKET)
    }

    #[test]
    fn espectador_com_ingresso_recebe_video_e_aviso_de_fim() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = new_server(&runtime);
        let ticket = new_watch_ticket();
        assert!(server.allow_viewer(7, ticket.clone()));

        runtime.block_on(async {
            let (mut stream, reply) = connect(server.port(), &ticket).await;
            assert_eq!(reply, Message::WatchAccepted);
            assert!(server.has_viewers());
            assert_eq!(server.viewers(), vec![7]);
            assert!(server.take_keyframe_request()); // quem entra pede keyframe

            // Antes do primeiro keyframe nada é enviado
            server.send_video(false, 1, Arc::new(vec![1]));
            server.send_video(true, 2, Arc::new(vec![2]));
            server.send_audio(3, Arc::new(vec![3]));

            match next_message(&mut stream).await {
                Some(Message::Video(frame)) => {
                    assert!(frame.keyframe);
                    assert_eq!(frame.timestamp_us, 2);
                }
                outra => panic!("esperava o keyframe, veio {outra:?}"),
            }
            match next_message(&mut stream).await {
                Some(Message::Audio(frame)) => assert_eq!(frame.timestamp_us, 3),
                outra => panic!("esperava o audio, veio {outra:?}"),
            }

            // O mesmo ingresso não vale duas vezes
            let (_other, reply) = connect(server.port(), &ticket).await;
            assert!(is_rejected_ticket(&reply));

            // Parar avisa quem assiste
            server.stop();
            assert_eq!(next_message(&mut stream).await, Some(Message::StreamStopped));
            assert_eq!(next_message(&mut stream).await, None);
        });
    }

    #[test]
    fn ingresso_desconhecido_e_recusado_e_revogar_derruba() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = new_server(&runtime);

        runtime.block_on(async {
            let (_stream, reply) = connect(server.port(), &new_watch_ticket()).await;
            assert!(is_rejected_ticket(&reply));
            assert!(!server.has_viewers());

            let ticket = new_watch_ticket();
            assert!(server.allow_viewer(3, ticket.clone()));
            let (mut stream, reply) = connect(server.port(), &ticket).await;
            assert_eq!(reply, Message::WatchAccepted);

            server.revoke_viewer(3);
            assert!(!server.has_viewers());
            assert_eq!(next_message(&mut stream).await, None); // a conexão foi encerrada
        });

        server.stop();
    }

    #[test]
    fn servidor_parado_nao_aceita_ingresso() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = new_server(&runtime);
        server.stop();
        assert!(!server.allow_viewer(1, new_watch_ticket()));
        assert!(!server.allow_viewer(1, "nao-e-hex".to_string()));
    }
}
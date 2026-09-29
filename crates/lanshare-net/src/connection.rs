//! Laço de uma conexão já aceita: Ping/Pong, Bye, mensagens e detecção de desconexão.

use lanshare_core::protocol::{
    read_message, write_message, Message, HEARTBEAT_INTERVAL, HEARTBEAT_TIMEOUT,
};
use std::fmt;
use std::io;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::interval;

/// Os quatro jeitos de uma conexão acabar.
#[derive(Debug)]
pub enum DisconnectReason {
    /// O outro lado avisou que ia sair (Bye).
    PeerLeft,
    /// Ficou tempo demais sem receber nada.
    Timeout,
    /// Erro de leitura/escrita (ex.: o outro programa foi fechado à força).
    ConnectionLost(io::Error),
    /// Este programa foi encerrado (Ctrl+C).
    LocalExit,
}

impl fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerLeft => write!(f, "saiu normalmente (enviou Bye)"),
            Self::Timeout => write!(f, "parou de responder (timeout)"),
            // Fim de arquivo ou reset (erro 10054 no Windows): o outro lado sumiu sem Bye
            Self::ConnectionLost(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                ) =>
            {
                write!(f, "conexao fechada sem aviso")
            }
            Self::ConnectionLost(e) => write!(f, "conexao perdida: {e}"),
            Self::LocalExit => write!(f, "encerrado por este programa"),
        }
    }
}

/// O que o run_connection avisa ao programa enquanto a conexão está viva.
#[derive(Debug)]
pub enum ConnectionEvent {
    /// Ping medido (tempo de ida e volta).
    Rtt(Duration),
    /// Mensagem do outro lado (tudo que não é Ping, Pong ou Bye).
    Message(Message),
}

/// Mantém a conexão viva até ela acabar e devolve o motivo.
///
/// - `shutdown`: quando vira `true`, mandamos Bye e saímos.
/// - `outgoing`: mensagens que o programa quer enviar ao outro lado.
/// - `on_event`: chamada a cada ping medido e a cada mensagem recebida.
///   Precisa ser rápida: ela roda dentro do laço da conexão.
pub async fn run_connection<F>(
    stream: TcpStream,
    mut shutdown: watch::Receiver<bool>,
    mut outgoing: mpsc::UnboundedReceiver<Message>,
    mut on_event: F,
) -> DisconnectReason
where
    F: FnMut(ConnectionEvent),
{
    let (mut read_half, mut write_half) = stream.into_split();

    // Uma tarefa só para ler; ela entrega o que chega por este canal.
    let (tx, mut rx) = mpsc::channel::<io::Result<Message>>(16);
    let reader = tokio::spawn(async move {
        loop {
            let result = read_message(&mut read_half).await;
            let failed = result.is_err();
            if tx.send(result).await.is_err() || failed {
                break;
            }
        }
    });

    let mut ticker = interval(HEARTBEAT_INTERVAL);
    let mut last_received = Instant::now();
    let mut next_id: u64 = 0;
    let mut pending_ping: Option<(u64, Instant)> = None;
    let mut outgoing_closed = false;

    let reason = loop {
        // select! espera as quatro coisas abaixo e executa a que acontecer primeiro.
        tokio::select! {
            // 1) Chegou algo do outro lado (ou a leitura falhou)
            incoming = rx.recv() => match incoming {
                Some(Ok(Message::Ping { id })) => {
                    last_received = Instant::now();
                    if let Err(e) = write_message(&mut write_half, &Message::Pong { id }).await {
                        break DisconnectReason::ConnectionLost(e);
                    }
                }
                Some(Ok(Message::Pong { id })) => {
                    last_received = Instant::now();
                    if let Some((sent_id, sent_at)) = pending_ping {
                        if sent_id == id {
                            on_event(ConnectionEvent::Rtt(sent_at.elapsed()));
                            pending_ping = None;
                        }
                    }
                }
                Some(Ok(Message::Bye)) => break DisconnectReason::PeerLeft,
                // Qualquer outra mensagem é entregue ao programa
                Some(Ok(other)) => {
                    last_received = Instant::now();
                    on_event(ConnectionEvent::Message(other));
                }
                Some(Err(e)) => break DisconnectReason::ConnectionLost(e),
                None => break DisconnectReason::ConnectionLost(io::ErrorKind::UnexpectedEof.into()),
            },

            // 2) Hora de mandar um Ping (e checar se o outro lado sumiu)
            _ = ticker.tick() => {
                if last_received.elapsed() > HEARTBEAT_TIMEOUT {
                    break DisconnectReason::Timeout;
                }
                next_id += 1;
                pending_ping = Some((next_id, Instant::now()));
                if let Err(e) = write_message(&mut write_half, &Message::Ping { id: next_id }).await {
                    break DisconnectReason::ConnectionLost(e);
                }
            },

            // 3) O programa quer enviar uma mensagem ao outro lado
            outgoing_msg = outgoing.recv(), if !outgoing_closed => match outgoing_msg {
                Some(message) => {
                    if let Err(e) = write_message(&mut write_half, &message).await {
                        break DisconnectReason::ConnectionLost(e);
                    }
                }
                // Ninguém mais pode enviar: desliga esta opção do select!
                None => outgoing_closed = true,
            },

            // 4) O programa está sendo encerrado: avisa com Bye
            _ = shutdown.changed() => {
                let _ = write_message(&mut write_half, &Message::Bye).await;
                break DisconnectReason::LocalExit;
            },
        }
    };

    reader.abort();
    reason
}
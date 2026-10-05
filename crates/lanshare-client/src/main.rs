//! Programa do Client: conecta no Host, mantém a conexão e acompanha quem está na sala.

use lanshare_core::protocol::{
    read_message, write_message, Message, UserInfo, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_core::settings;
use lanshare_net::{run_connection, ConnectionEvent};
use std::collections::HashMap;
use std::io;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> io::Result<()> {
    // Lê o nome do settings.json (ou pergunta na primeira vez e salva)
    let username = settings::get_or_ask_username();

    let address = match std::env::args().nth(1) {
        Some(entrada) => normalize_address(&entrada),
        None => format!("127.0.0.1:{DEFAULT_PORT}"),
    };

    println!("[client] Conectando em {address}...");

    let mut stream = match timeout(CONNECT_TIMEOUT, TcpStream::connect(&address)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(erro)) => {
            println!("[client] Nao foi possivel conectar: {erro}");
            std::process::exit(1);
        }
        Err(_) => {
            println!(
                "[client] Tempo esgotado ({}s). Confira o IP, o Firewall do Host e se o Host esta rodando.",
                CONNECT_TIMEOUT.as_secs()
            );
            std::process::exit(1);
        }
    };

    // Se o Host recusou, o motivo já foi impresso dentro do handshake
    let Some((client_id, display_name, everyone)) = handshake(&mut stream, &username).await?
    else {
        std::process::exit(1);
    };
    println!("[client] Voce entrou como '{display_name}' (#{client_id}). Ctrl+C para sair.");

    // Tabela dos OUTROS usuários (sem nós mesmos), atualizada pelos avisos do Host
    let mut users: HashMap<u64, String> = everyone
        .into_iter()
        .filter(|u| u.id != client_id)
        .map(|u| (u.id, u.name))
        .collect();
    print_users(&users);

    // Ctrl+C vira um aviso de "encerrar" para o run_connection mandar o Bye
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);
    });

    // Por enquanto o Client não envia nada além de Ping/Bye, mas a caixa de saída
    // precisa existir (e o `_outgoing_tx` fica vivo até o fim da função).
    let (_outgoing_tx, outgoing_rx) = mpsc::unbounded_channel::<Message>();

    let mut ping_count: u32 = 0;
    let reason = run_connection(stream, shutdown_rx, outgoing_rx, |event| match event {
        ConnectionEvent::Rtt(rtt) => {
            // Um ping a cada 2 s seria barulho demais: mostra 1 a cada 10
            if ping_count % 10 == 0 {
                println!("[client] ping: {} ms", rtt.as_millis());
            }
            ping_count += 1;
        }
        ConnectionEvent::Message(message) => handle_message(message, client_id, &mut users),
    })
    .await;

    println!("[client] Desconectado do Host: {reason}");
    Ok(())
}

/// Manda o Hello (com o nome de usuário) e lê a resposta.
/// Devolve (nosso ID, nosso nome, todos na sala) ou None se fomos recusados.
async fn handshake(
    stream: &mut TcpStream,
    username: &str,
) -> io::Result<Option<(u64, String, Vec<UserInfo>)>> {
    // O campo do protocolo ainda se chama device_name, mas agora leva o nome de usuário
    let hello = Message::Hello {
        protocol_version: PROTOCOL_VERSION,
        device_name: username.to_string(),
    };
    write_message(stream, &hello).await?;

    match read_message(stream).await? {
        Message::Welcome {
            client_id,
            display_name,
            users,
        } => Ok(Some((client_id, display_name, users))),

        Message::Rejected { reason } => {
            println!("[client] Host recusou: {reason}");
            Ok(None)
        }

        outra => {
            println!("[client] Resposta inesperada: {outra:?}");
            Ok(None)
        }
    }
}

/// Trata as mensagens do Host sobre quem entra e sai.
/// Só mostra "entrou"/"saiu" quando a tabela realmente mudou (evita repetição).
fn handle_message(message: Message, my_id: u64, users: &mut HashMap<u64, String>) {
    match message {
        Message::UserJoined { user } => {
            if user.id != my_id && users.insert(user.id, user.name.clone()).is_none() {
                println!(
                    "[client] {} (#{}) entrou - {} na sala",
                    user.name,
                    user.id,
                    users.len() + 1
                );
            }
        }

        Message::UserLeft { user } => {
            if users.remove(&user.id).is_some() {
                println!(
                    "[client] {} (#{}) saiu - {} na sala",
                    user.name,
                    user.id,
                    users.len() + 1
                );
            }
        }

        outra => println!("[client] Mensagem inesperada: {outra:?}"),
    }
}

/// Mostra os outros usuários conectados.
fn print_users(users: &HashMap<u64, String>) {
    if users.is_empty() {
        println!("[client] Nenhum outro usuario conectado.");
        return;
    }

    let mut list: Vec<_> = users.iter().collect();
    list.sort_by_key(|(id, _)| **id);

    println!("[client] Outros usuarios conectados ({}):", list.len());
    for (id, name) in list {
        println!("[client]   #{id} {name}");
    }
}

/// Aceita "192.168.0.10" ou "192.168.0.10:47800" (só IPv4 por enquanto).
fn normalize_address(entrada: &str) -> String {
    if entrada.contains(':') {
        entrada.to_string()
    } else {
        format!("{entrada}:{DEFAULT_PORT}")
    }
}
//! Programa do Client: conecta no Host e mantém a conexão.

use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_net::run_connection;
use std::io;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> io::Result<()> {
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

    if !handshake(&mut stream).await? {
        std::process::exit(1);
    }
    println!("[client] Host aceitou a conexao. Ctrl+C para sair.");

    // Ctrl+C vira um aviso de "encerrar" para o run_connection mandar o Bye
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);
    });

    let reason = run_connection(stream, shutdown_rx, |rtt| {
        println!("[client] ping: {} ms", rtt.as_millis());
    })
    .await;

    println!("[client] Desconectado do Host: {reason}");
    Ok(())
}

/// Manda o Hello e lê a resposta. Devolve true se o Host aceitou.
async fn handshake(stream: &mut TcpStream) -> io::Result<bool> {
    let hello = Message::Hello {
        protocol_version: PROTOCOL_VERSION,
        device_name: device_name(),
    };
    write_message(stream, &hello).await?;

    match read_message(stream).await? {
        Message::HelloAck { accepted: true, .. } => Ok(true),
        Message::HelloAck {
            accepted: false,
            reason,
        } => {
            println!("[client] Host recusou: {}", reason.unwrap_or_default());
            Ok(false)
        }
        outra => {
            println!("[client] Resposta inesperada: {outra:?}");
            Ok(false)
        }
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

/// Nome deste computador (no Windows fica em COMPUTERNAME).
fn device_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "dispositivo-desconhecido".to_string())
}
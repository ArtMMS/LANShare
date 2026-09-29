use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Quanto tempo esperamos o Host responder antes de desistir.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Sem argumento, conecta neste próprio computador
    let address = match std::env::args().nth(1) {
        Some(entrada) => normalize_address(&entrada),
        None => format!("127.0.0.1:{DEFAULT_PORT}"),
    };

    println!("[client] Conectando em {address}...");

    // Tenta conectar, mas desiste depois de CONNECT_TIMEOUT
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
    println!("[client] Conectado!");

    let hello = Message::Hello {
        protocol_version: PROTOCOL_VERSION,
        device_name: device_name(),
    };
    write_message(&mut stream, &hello).await?;

    match read_message(&mut stream).await? {
        Message::HelloAck { accepted: true, .. } => {
            println!("[client] Host aceitou a conexao.");
        }
        Message::HelloAck {
            accepted: false,
            reason,
        } => {
            println!("[client] Host recusou: {}", reason.unwrap_or_default());
        }
        outra => {
            println!("[client] Resposta inesperada: {outra:?}");
        }
    }

    Ok(())
}

fn normalize_address(entrada: &str) -> String {
    if entrada.contains(':') {
        entrada.to_string()
    } else {
        format!("{entrada}:{DEFAULT_PORT}")
    }
}

/// Nome deste computador, para o Host saber quem chegou.
/// No Windows, o nome fica na variável de ambiente COMPUTERNAME.
fn device_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "dispositivo-desconhecido".to_string())
}
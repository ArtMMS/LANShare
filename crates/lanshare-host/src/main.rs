//! Programa do Host: fica escutando e aceita Clients.

use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_net::run_connection;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> io::Result<()> {
    let address = format!("0.0.0.0:{DEFAULT_PORT}");
    let listener = TcpListener::bind(&address).await?;
    println!("[host] Escutando em {address}");
    print_local_addresses();
    println!("[host] Ctrl+C para encerrar.");

    // Canal usado para mandar "encerrar" a todas as conexões de uma vez
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, client_addr) = accepted?;
                let shutdown = shutdown_rx.clone();
                tokio::spawn(handle_client(stream, client_addr, shutdown));
            },

            _ = tokio::signal::ctrl_c() => {
                println!("[host] Encerrando... avisando os clients.");
                let _ = shutdown_tx.send(true);
                // Dá um tempinho para os Bye saírem antes de fechar
                tokio::time::sleep(Duration::from_millis(500)).await;
                break;
            },
        }
    }

    Ok(())
}

/// Cuida de UM Client do começo ao fim.
async fn handle_client(mut stream: TcpStream, addr: SocketAddr, shutdown: watch::Receiver<bool>) {
    let name = match handshake(&mut stream).await {
        Ok(Some(name)) => name,
        Ok(None) => return, // recusado; o motivo já foi impresso
        Err(erro) => {
            println!("[host] Falha no handshake com {addr}: {erro}");
            return;
        }
    };

    println!("[host] {name} entrou ({addr})");

    let reason = run_connection(stream, shutdown, |rtt| {
        println!("[host] ping de {name}: {} ms", rtt.as_millis());
    })
    .await;

    println!("[host] {name} saiu ({addr}): {reason}");
}

/// Espera o Hello e responde. Devolve o nome do Client se ele foi aceito.
async fn handshake(stream: &mut TcpStream) -> io::Result<Option<String>> {
    match read_message(stream).await? {
        Message::Hello {
            protocol_version,
            device_name,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                let resposta = Message::HelloAck {
                    accepted: false,
                    reason: Some(format!(
                        "versao do protocolo incompativel (host usa v{PROTOCOL_VERSION})"
                    )),
                };
                write_message(stream, &resposta).await?;
                println!("[host] {device_name} recusado: versao incompativel");
                return Ok(None);
            }

            let resposta = Message::HelloAck {
                accepted: true,
                reason: None,
            };
            write_message(stream, &resposta).await?;
            Ok(Some(device_name))
        }

        outra => {
            println!("[host] Mensagem inesperada no inicio: {outra:?}");
            Ok(None)
        }
    }
}

/// Mostra os IPs deste computador, para você saber o que digitar no Client.
fn print_local_addresses() {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => {
            println!("[host] Use um destes enderecos no Client:");
            for interface in interfaces {
                if let IpAddr::V4(ip) = interface.ip() {
                    if !interface.is_loopback() && !ip.is_link_local() {
                        println!("[host]   {ip}   ({})", interface.name);
                    }
                }
            }
        }
        Err(erro) => println!("[host] Nao foi possivel listar os enderecos: {erro}"),
    }
}
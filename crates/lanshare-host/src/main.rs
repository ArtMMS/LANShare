//! Programa do Host: fica escutando e aceita vários Clients ao mesmo tempo.

mod registry;

use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_net::run_connection;
use registry::{ClientInfo, Registry};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Limite de Clients. None = sem limite; Some(8) = no máximo 8.
/// (Vai virar uma opção do Host na GUI.)
const MAX_CLIENTS: Option<usize> = None;

#[tokio::main]
async fn main() -> io::Result<()> {
    let address = format!("0.0.0.0:{DEFAULT_PORT}");
    let listener = TcpListener::bind(&address).await?;

    match MAX_CLIENTS {
        Some(max) => println!("[host] Escutando em {address} (maximo {max} clients)"),
        None => println!("[host] Escutando em {address} (sem limite de clients)"),
    }
    print_local_addresses();
    println!("[host] Ctrl+C para encerrar.");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let registry = Registry::new(MAX_CLIENTS);

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, client_addr) = accepted?;
                tokio::spawn(handle_client(
                    stream,
                    client_addr,
                    shutdown_rx.clone(),
                    registry.clone(),
                ));
            },

            _ = tokio::signal::ctrl_c() => {
                println!("[host] Encerrando... avisando os clients.");
                let _ = shutdown_tx.send(true);
                tokio::time::sleep(Duration::from_millis(500)).await;
                break;
            },
        }
    }

    Ok(())
}

/// Cuida de UM Client do começo ao fim.
async fn handle_client(
    mut stream: TcpStream,
    addr: SocketAddr,
    shutdown: watch::Receiver<bool>,
    registry: Registry,
) {
    let client = match handshake(&mut stream, addr, &registry).await {
        Ok(Some(client)) => client,
        Ok(None) => return, // recusado; o motivo já foi impresso
        Err(erro) => {
            println!("[host] Falha no handshake com {addr}: {erro}");
            return;
        }
    };
    let (id, name) = (client.id, client.name);

    println!("[host] {name} (#{id}) entrou ({addr})");
    print_roster(&registry);

    // O ping é medido, mas o Host não imprime (a GUI vai mostrar depois)
    let reason = run_connection(stream, shutdown, |_rtt| {}).await;

    registry.remove(id);
    println!("[host] {name} (#{id}) saiu ({addr}): {reason}");
    print_roster(&registry);
}

/// Espera o Hello, registra o Client e responde com Welcome ou Rejected.
async fn handshake(
    stream: &mut TcpStream,
    addr: SocketAddr,
    registry: &Registry,
) -> io::Result<Option<ClientInfo>> {
    match read_message(stream).await? {
        Message::Hello {
            protocol_version,
            device_name,
        } => {
            if protocol_version != PROTOCOL_VERSION {
                let motivo =
                    format!("versao do protocolo incompativel (host usa v{PROTOCOL_VERSION})");
                return refuse(stream, &device_name, motivo).await;
            }

            // Tenta registrar; None = sala cheia
            let Some(client) = registry.try_add(&device_name, addr) else {
                return refuse(stream, &device_name, "sala cheia".to_string()).await;
            };

            // O nome final pode ser diferente do pedido (limpeza ou repetido)
            if client.name != device_name {
                println!("[host] Nome '{device_name}' ajustado para '{}'", client.name);
            }

            let resposta = Message::Welcome {
                client_id: client.id,
                display_name: client.name.clone(),
            };
            // Se não deu para responder, desfaz o registro
            if let Err(erro) = write_message(stream, &resposta).await {
                registry.remove(client.id);
                return Err(erro);
            }

            Ok(Some(client))
        }

        outra => {
            println!("[host] Mensagem inesperada no inicio: {outra:?}");
            Ok(None)
        }
    }
}

/// Responde "recusado" ao Client e avisa no log.
async fn refuse(
    stream: &mut TcpStream,
    name: &str,
    reason: String,
) -> io::Result<Option<ClientInfo>> {
    println!("[host] {name} recusado: {reason}");
    write_message(stream, &Message::Rejected { reason }).await?;
    Ok(None)
}

/// Mostra quem está conectado agora.
fn print_roster(registry: &Registry) {
    let clients = registry.list();
    if clients.is_empty() {
        println!("[host] Ninguem conectado.");
        return;
    }

    println!("[host] Conectados agora ({}):", clients.len());
    for c in clients {
        println!(
            "[host]   #{} {} - {} - ha {}s",
            c.id,
            c.name,
            c.addr,
            c.connected_at.elapsed().as_secs()
        );
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
use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use std::net::IpAddr;
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // "0.0.0.0" significa "aceite conexões vindas de qualquer placa de rede".
    // Assim outros computadores da LAN conseguem chegar até o Host.
    let address = format!("0.0.0.0:{DEFAULT_PORT}");
    let listener = TcpListener::bind(&address).await?;
    println!("[host] Escutando em {address}");
    print_local_addresses();

    // fica esperando Clients para sempre.
    loop {
        // "accept" espera até alguém conectar. Devolve a conexão (stream)
        // e o endereço de quem conectou.
        let (stream, client_addr) = listener.accept().await?;
        println!("[host] Nova conexao de {client_addr}");

        tokio::spawn(async move {
            if let Err(erro) = handle_client(stream).await {
                println!("[host] Erro com {client_addr}: {erro}");
            }
            println!("[host] {client_addr} desconectou");
        });
    }
}

/// Mostra os IPs deste computador, para você saber o que digitar no Client.
fn print_local_addresses() {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => {
            println!("[host] Use um destes enderecos no Client:");
            for interface in interfaces {
                // Só IPv4, sem loopback (127.x) e sem endereços "169.254.x" (rede sem DHCP)
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


/// Cuida de UM Client: espera o Hello e responde com um HelloAck.
async fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    let primeira = read_message(&mut stream).await?;

    match primeira {
        Message::Hello {
            protocol_version,
            device_name,
        } => {
            println!("[host] Hello de '{device_name}' (protocolo v{protocol_version})");

            if protocol_version != PROTOCOL_VERSION {
                let resposta = Message::HelloAck {
                    accepted: false,
                    reason: Some(format!(
                        "versao do protocolo incompativel (host usa v{PROTOCOL_VERSION})"
                    )),
                };
                write_message(&mut stream, &resposta).await?;
                return Ok(());
            }

            let resposta = Message::HelloAck {
                accepted: true,
                reason: None,
            };
            write_message(&mut stream, &resposta).await?;
            println!("[host] Cliente '{device_name}' aceito");
        }

        outra => {
            println!("[host] Mensagem inesperada no inicio: {outra:?}");
        }
    }

    Ok(())
}
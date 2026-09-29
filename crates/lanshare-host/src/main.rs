use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // "0.0.0.0" significa "aceite conexões vindas de qualquer placa de rede".
    // Assim outros computadores da LAN conseguem chegar até o Host.
    let address = format!("0.0.0.0:{DEFAULT_PORT}");

    let listener = TcpListener::bind(&address).await?;
    println!("[host] Escutando em {address}");

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

            // Tudo certo: aceita.
            let resposta = Message::HelloAck {
                accepted: true,
                reason: None,
            };
            write_message(&mut stream, &resposta).await?;
            println!("[host] Cliente '{device_name}' aceito");
        }

        // Qualquer outra mensagem no começo é um erro do Client.
        outra => {
            println!("[host] Mensagem inesperada no inicio: {outra:?}");
        }
    }

    Ok(())
}
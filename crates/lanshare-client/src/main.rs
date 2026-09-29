use lanshare_core::protocol::{
    read_message, write_message, Message, DEFAULT_PORT, PROTOCOL_VERSION,
};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Endereço do Host. Se você passar um argumento ao rodar (ex.: 192.168.0.10:47800),
    // usamos ele. Se não passar nada, usamos o próprio computador (127.0.0.1).
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("127.0.0.1:{DEFAULT_PORT}"));

    println!("[client] Conectando em {address}...");

    // Abre a conexão TCP. Se o Host não estiver lá, aqui dá erro.
    let mut stream = TcpStream::connect(&address).await?;
    println!("[client] Conectado!");

    // Passo 1 da conversa: mandar o Hello.
    let hello = Message::Hello {
        protocol_version: PROTOCOL_VERSION,
        device_name: device_name(),
    };
    write_message(&mut stream, &hello).await?;

    // Passo 2 da conversa: esperar a resposta do Host.
    match read_message(&mut stream).await? {
        Message::HelloAck { accepted: true, .. } => {
            println!("[client] Host aceitou a conexao.");
        }
        Message::HelloAck {
            accepted: false,
            reason,
        } => {
            // "unwrap_or_default" usa um texto vazio se não houver motivo.
            println!(
                "[client] Host recusou: {}",
                reason.unwrap_or_default()
            );
        }
        outra => {
            println!("[client] Resposta inesperada: {outra:?}");
        }
    }

    Ok(())
}

/// Nome deste computador, para o Host saber quem chegou.
/// No Windows, o nome fica na variável de ambiente COMPUTERNAME.
fn device_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "dispositivo-desconhecido".to_string())
}
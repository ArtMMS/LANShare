//! Programa do Host: fica escutando e aceita vários Clients ao mesmo tempo.

mod registry;

use lanshare_core::protocol::{
    read_message, write_message, Message, UserInfo, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_net::run_connection;
use registry::{ClientInfo, Registry};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::sync::watch;

/// Limite de Clients. None = sem limite; Some(8) = no máximo 8.
/// (Vai virar uma opção do Host na GUI.)
const MAX_CLIENTS: Option<usize> = None;

/// Tentativas de senha por conexão.
const MAX_PASSWORD_ATTEMPTS: u32 = 3;

/// Tempo que o Client tem para responder com a senha.
const PASSWORD_TIMEOUT: Duration = Duration::from_secs(30);

/// Pausa depois de uma senha errada (atrapalha tentativas em sequência).
const WRONG_PASSWORD_DELAY: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> io::Result<()> {
    // Pergunta a senha antes de abrir a sala (Enter vazio = sem senha)
    let password = ask_room_password()?;

    let address = format!("0.0.0.0:{DEFAULT_PORT}");
    let listener = TcpListener::bind(&address).await?;

    match MAX_CLIENTS {
        Some(max) => println!("[host] Escutando em {address} (maximo {max} clients)"),
        None => println!("[host] Escutando em {address} (sem limite de clients)"),
    }
    if password.is_some() {
        println!("[host] Sala protegida por senha.");
    } else {
        println!("[host] Sala sem senha.");
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
                    password.clone(),
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

/// Pergunta a senha da sala sem mostrar o que é digitado.
/// Vazio (ou só espaços) = sala sem senha. A senha fica só na memória.
fn ask_room_password() -> io::Result<Option<Arc<String>>> {
    let typed = rpassword::prompt_password("[host] Senha da sala (Enter para sem senha): ")?;

    if typed.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(Arc::new(typed)))
    }
}

/// Cuida de UM Client do começo ao fim.
async fn handle_client(
    mut stream: TcpStream,
    addr: SocketAddr,
    shutdown: watch::Receiver<bool>,
    registry: Registry,
    password: Option<Arc<String>>,
) {
    // Caixa de saída desta conexão: quem quiser falar com este Client põe aqui
    let (out_tx, out_rx) = mpsc::unbounded_channel::<Message>();

    let client = match handshake(&mut stream, addr, &registry, &password, out_tx).await {
        Ok(Some(client)) => client,
        Ok(None) => return, // recusado; o motivo já foi impresso
        Err(erro) => {
            println!("[host] Falha no handshake com {addr}: {erro}");
            return;
        }
    };
    let (id, name) = (client.id, client.name);

    println!("[host] {name} (#{id}) entrou ({addr})");

    // Avisa os outros Clients que alguém entrou
    let joined = Message::UserJoined {
        user: UserInfo {
            id,
            name: name.clone(),
        },
    };
    registry.broadcast(&joined, Some(id));
    print_roster(&registry);

    // Guardamos uma cópia para saber, no fim, se o Host inteiro está encerrando
    let shutdown_check = shutdown.clone();

    // O ping é medido, mas o Host não imprime (a GUI vai mostrar depois)
    let reason = run_connection(stream, shutdown, out_rx, |_event| {}).await;

    registry.remove(id);

    // No encerramento do Host todo mundo sai junto; não vale avisar um a um
    if !*shutdown_check.borrow() {
        let left = Message::UserLeft {
            user: UserInfo {
                id,
                name: name.clone(),
            },
        };
        registry.broadcast(&left, None);
    }

    println!("[host] {name} (#{id}) saiu ({addr}): {reason}");
    print_roster(&registry);
}

/// Espera o Hello, confere a senha (se houver), registra o Client
/// e responde com Welcome ou Rejected.
async fn handshake(
    stream: &mut TcpStream,
    addr: SocketAddr,
    registry: &Registry,
    password: &Option<Arc<String>>,
    out_tx: UnboundedSender<Message>,
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

            // Senha primeiro: quem não passa nunca chega a ser registrado
            if !check_password(stream, addr, &device_name, password).await? {
                return Ok(None);
            }

            // Tenta registrar; None = sala cheia
            let Some(client) = registry.try_add(&device_name, addr, out_tx) else {
                return refuse(stream, &device_name, "sala cheia".to_string()).await;
            };

            // O nome final pode ser diferente do pedido (limpeza ou repetido)
            if client.name != device_name {
                println!("[host] Nome '{device_name}' ajustado para '{}'", client.name);
            }

            let resposta = Message::Welcome {
                client_id: client.id,
                display_name: client.name.clone(),
                users: registry.users(),
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

/// Se a sala tem senha, pede ao Client (até MAX_PASSWORD_ATTEMPTS vezes).
/// Devolve true se pode entrar; false se foi recusado (a recusa já foi enviada).
async fn check_password(
    stream: &mut TcpStream,
    addr: SocketAddr,
    name: &str,
    password: &Option<Arc<String>>,
) -> io::Result<bool> {
    let Some(expected) = password else {
        return Ok(true); // sala sem senha
    };

    for attempt in 1..=MAX_PASSWORD_ATTEMPTS {
        write_message(stream, &Message::PasswordRequired).await?;

        let reply = match tokio::time::timeout(PASSWORD_TIMEOUT, read_message(stream)).await {
            Ok(reply) => reply?,
            Err(_) => {
                refuse(stream, name, "tempo esgotado esperando a senha".to_string()).await?;
                return Ok(false);
            }
        };

        match reply {
            Message::Password { password } if password == **expected => return Ok(true),

            Message::Password { .. } => {
                println!(
                    "[host] {name} ({addr}) errou a senha (tentativa {attempt}/{MAX_PASSWORD_ATTEMPTS})"
                );
                tokio::time::sleep(WRONG_PASSWORD_DELAY).await;
            }

            outra => {
                println!("[host] Mensagem inesperada ao pedir a senha: {outra:?}");
                return Ok(false);
            }
        }
    }

    refuse(stream, name, "senha incorreta".to_string()).await?;
    Ok(false)
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
//! Programa do Host: fica escutando e aceita vários Clients ao mesmo tempo.

mod capture;
mod discovery;
mod kick;
mod registry;

use kick::Kicker;
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
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{oneshot, watch};

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
    let kicker = Kicker::new();

    // Responde aos Clients que procuram salas na rede (UDP)
    tokio::spawn(discovery::run_responder(
        registry.clone(),
        password.is_some(),
        MAX_CLIENTS,
    ));

    // Comandos digitados no terminal do Host
    let mut commands = spawn_command_reader();
    println!("[host] Comandos: list | kick <id> | capture [segundos] | help");

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
                    kicker.clone(),
                ));
            },

            Some(line) = commands.recv() => {
                handle_command(&line, &registry, &kicker);
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

/// Lê o que o Host digita no terminal (numa thread própria, porque ler o teclado bloqueia).
fn spawn_command_reader() -> UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();

    std::thread::spawn(move || {
        for line in io::stdin().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    rx
}

/// Executa um comando digitado pelo Host.
fn handle_command(line: &str, registry: &Registry, kicker: &Kicker) {
    let mut parts = line.split_whitespace();

    match parts.next() {
        Some("list") => print_roster(registry),

        Some("kick") => {
            // Aceita "kick 3" e "kick #3"
            let id = parts
                .next()
                .and_then(|text| text.trim_start_matches('#').parse::<u64>().ok());

            match id {
                Some(id) => kick_client(id, registry, kicker),
                None => println!("[host] Uso: kick <id>   (veja os ids com 'list')"),
            }
        }

        Some("capture") => {
            // "capture" = 5 segundos; "capture 10" = 10 segundos
            let seconds = parts
                .next()
                .and_then(|text| text.parse::<u64>().ok())
                .filter(|&s| s > 0)
                .unwrap_or(5);
            start_capture_test(seconds);
        }

        Some("help") => print_help(),

        Some(outro) => println!("[host] Comando desconhecido: '{outro}'. Digite 'help'."),

        None => {}
    }
}

/// Teste temporário da captura de tela.
/// Roda numa thread própria para não travar o Host enquanto captura.
fn start_capture_test(seconds: u64) {
    println!("[host] Capturando a tela por {seconds}s...");

    std::thread::spawn(move || {
        if let Err(erro) = capture::test_capture(seconds) {
            println!("[host] Falha na captura: {erro}");
        }
    });
}

/// Remove um Client: avisa ele, avisa os outros e encerra a conexão dele.
fn kick_client(id: u64, registry: &Registry, kicker: &Kicker) {
    let Some(target) = registry.list().into_iter().find(|c| c.id == id) else {
        println!("[host] Nao existe client com id #{id}. Use 'list'.");
        return;
    };

    if !kicker.kick(id, "Voce foi removido pelo Host") {
        println!("[host] {} (#{id}) ja esta saindo.", target.name);
        return;
    }

    println!("[host] Removendo {} (#{id})...", target.name);

    // Avisa todos os outros (o expulso já recebeu o aviso dele)
    let notice = Message::UserKicked {
        user: UserInfo {
            id,
            name: target.name.clone(),
        },
    };
    registry.broadcast(&notice, Some(id));
}

fn print_help() {
    println!("[host] Comandos:");
    println!("[host]   list              mostra quem esta conectado");
    println!("[host]   kick <id>         remove o client com esse id (veja o id em 'list')");
    println!("[host]   capture [segs]    teste de captura da tela (padrao: 5 segundos)");
    println!("[host]   help              mostra esta ajuda");
}

/// Cuida de UM Client do começo ao fim.
async fn handle_client(
    mut stream: TcpStream,
    addr: SocketAddr,
    shutdown: watch::Receiver<bool>,
    registry: Registry,
    password: Option<Arc<String>>,
    kicker: Kicker,
) {
    // Caixa de saída desta conexão: quem quiser falar com este Client põe aqui
    let (out_tx, out_rx) = mpsc::unbounded_channel::<Message>();
    let kick_out_tx = out_tx.clone();

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

    // Permite ao Host expulsar este Client (comando kick)
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    kicker.register(id, kick_out_tx, stop_tx);

    // Sinal de encerramento SÓ desta conexão: dispara no kick ou no encerramento do Host
    let (conn_shutdown_tx, conn_shutdown_rx) = watch::channel(false);
    let mut global_shutdown = shutdown.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = global_shutdown.changed() => {},
            _ = stop_rx => {},
            _ = conn_shutdown_tx.closed() => return, // a conexão já acabou sozinha
        }
        let _ = conn_shutdown_tx.send(true);
    });

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
    let reason = run_connection(stream, conn_shutdown_rx, out_rx, |_event| {}).await;

    registry.remove(id);
    let was_kicked = kicker.finish(id);

    // No encerramento do Host todo mundo sai junto; e quem foi expulso já foi anunciado
    if !*shutdown_check.borrow() && !was_kicked {
        let left = Message::UserLeft {
            user: UserInfo {
                id,
                name: name.clone(),
            },
        };
        registry.broadcast(&left, None);
    }

    if was_kicked {
        println!("[host] {name} (#{id}) foi removido ({addr})");
    } else {
        println!("[host] {name} (#{id}) saiu ({addr}): {reason}");
    }
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
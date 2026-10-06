//! Programa do Client: acha uma sala, conecta no Host e acompanha quem está na sala.

mod discovery;

use lanshare_core::protocol::{
    read_message, write_message, Message, UserInfo, VideoFrame, DEFAULT_PORT, PROTOCOL_VERSION,
};
use lanshare_core::settings;
use lanshare_net::{run_connection, ConnectionEvent};
use std::collections::HashMap;
use std::io::{self, Write};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// De quanto em quanto tempo o Client mostra o resumo do vídeo recebido.
const VIDEO_REPORT_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> io::Result<()> {
    // Lê o nome do settings.json (ou pergunta na primeira vez e salva)
    let username = settings::get_or_ask_username();

    // Atalho: um IP na linha de comando pula a pergunta
    let address = match std::env::args().nth(1) {
        Some(entrada) => normalize_address(&entrada),
        None => choose_address().await?,
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

    // Sem isso o TCP junta pacotes pequenos e atrasa o vídeo
    let _ = stream.set_nodelay(true);

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
    let mut video_stats = VideoStats::new();
    let reason = run_connection(stream, shutdown_rx, outgoing_rx, |event| match event {
        ConnectionEvent::Rtt(rtt) => {
            // Um ping a cada 2 s seria barulho demais: mostra 1 a cada 10
            if ping_count % 10 == 0 {
                println!("[client] ping: {} ms", rtt.as_millis());
            }
            ping_count += 1;
        }
        ConnectionEvent::Message(message) => {
            handle_message(message, client_id, &mut users, &mut video_stats)
        }
    })
    .await;

    println!("[client] Desconectado do Host: {reason}");
    Ok(())
}

/// Conta o vídeo que chega e mostra um resumo de tempos em tempos.
/// (Mostrar a imagem de verdade é o próximo passo.)
struct VideoStats {
    receiving: bool,
    since: Instant,
    frames: u32,
    keyframes: u32,
    bytes: u64,
}

impl VideoStats {
    fn new() -> Self {
        Self {
            receiving: false,
            since: Instant::now(),
            frames: 0,
            keyframes: 0,
            bytes: 0,
        }
    }

    fn record(&mut self, frame: &VideoFrame) {
        if !self.receiving {
            self.receiving = true;
            self.since = Instant::now();
            println!("[client] Recebendo video do Host.");
        }

        self.frames += 1;
        self.bytes += frame.data.len() as u64;
        if frame.keyframe {
            self.keyframes += 1;
        }

        let elapsed = self.since.elapsed();
        if elapsed >= VIDEO_REPORT_INTERVAL {
            let secs = elapsed.as_secs_f64();
            println!(
                "[client] Video: {:.1} FPS, {:.2} Mbps, {} keyframe(s) em {:.0}s",
                self.frames as f64 / secs,
                self.bytes as f64 * 8.0 / secs / 1_000_000.0,
                self.keyframes,
                secs
            );

            self.since = Instant::now();
            self.frames = 0;
            self.keyframes = 0;
            self.bytes = 0;
        }
    }
}

/// Procura salas na rede, mostra a lista e pergunta qual usar.
/// Aceita o número de uma sala da lista ou um IP digitado; Enter procura de novo.
async fn choose_address() -> io::Result<String> {
    loop {
        println!("[client] Procurando salas na rede...");
        let rooms = match discovery::scan().await {
            Ok(rooms) => rooms,
            Err(erro) => {
                println!("[client] Nao foi possivel procurar salas: {erro}");
                Vec::new()
            }
        };
        print_rooms(&rooms);

        let what = if rooms.is_empty() {
            "Digite o IP do Host"
        } else {
            "Numero da sala ou IP"
        };
        print!("[client] {what} (Enter para procurar de novo): ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "entrada fechada",
            ));
        }
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        // Só dígitos = número da sala na lista
        if let Ok(number) = input.parse::<usize>() {
            if (1..=rooms.len()).contains(&number) {
                return Ok(rooms[number - 1].address());
            }
            println!("[client] A sala {number} nao existe na lista.");
            continue;
        }

        // Qualquer outra coisa = IP digitado
        return Ok(normalize_address(input));
    }
}

/// Mostra as salas encontradas.
fn print_rooms(rooms: &[discovery::FoundRoom]) {
    if rooms.is_empty() {
        println!("[client] Nenhuma sala encontrada na rede.");
        return;
    }

    println!("[client] Salas encontradas:");
    for (index, room) in rooms.iter().enumerate() {
        let info = &room.info;

        let people = match info.max_users {
            Some(max) => format!("{}/{}", info.users, max),
            None => info.users.to_string(),
        };
        let lock = if info.has_password { " - com senha" } else { "" };
        let version = if info.protocol_version != PROTOCOL_VERSION {
            " - versao incompativel"
        } else {
            ""
        };

        println!(
            "[client]   {}) {} - {} - {} na sala{}{}",
            index + 1,
            info.name,
            room.address(),
            people,
            lock,
            version
        );
    }
}

/// Manda o Hello (com o nome de usuário), responde ao pedido de senha (se houver)
/// e lê a resposta final.
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

    let mut asked_before = false;

    loop {
        match read_message(stream).await? {
            Message::Welcome {
                client_id,
                display_name,
                users,
            } => return Ok(Some((client_id, display_name, users))),

            Message::PasswordRequired => {
                if asked_before {
                    println!("[client] Senha incorreta. Tente novamente.");
                } else {
                    println!("[client] Esta sala tem senha.");
                }
                asked_before = true;

                let password = ask_password().await?;
                write_message(stream, &Message::Password { password }).await?;
            }

            Message::Rejected { reason } => {
                println!("[client] Host recusou: {reason}");
                return Ok(None);
            }

            outra => {
                println!("[client] Resposta inesperada: {outra:?}");
                return Ok(None);
            }
        }
    }
}

/// Lê a senha sem mostrar o que é digitado (fora da thread principal do tokio).
async fn ask_password() -> io::Result<String> {
    tokio::task::spawn_blocking(|| rpassword::prompt_password("[client] Senha da sala: "))
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
}

/// Trata as mensagens do Host: vídeo, e quem entra, sai ou é removido.
/// Só mostra o aviso quando a tabela realmente mudou (evita repetição).
fn handle_message(
    message: Message,
    my_id: u64,
    users: &mut HashMap<u64, String>,
    video: &mut VideoStats,
) {
    match message {
        // Pacote de vídeo (por enquanto só contamos; a exibição é o próximo passo)
        Message::Video(frame) => video.record(&frame),

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

        // Outro usuário foi removido pelo Host
        Message::UserKicked { user } => {
            if users.remove(&user.id).is_some() {
                println!(
                    "[client] {} (#{}) foi removido pelo Host - {} na sala",
                    user.name,
                    user.id,
                    users.len() + 1
                );
            }
        }

        // Fomos nós: a conexão será encerrada em seguida pelo Host
        Message::Kicked { reason } => println!("[client] {reason}."),

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
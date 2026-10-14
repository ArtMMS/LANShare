//! Terminal do Host: pede a senha, abre a sala e executa os comandos digitados.
//!
//! É só uma "tela" para o motor (engine.rs): a interface gráfica usa o mesmo motor.

use crate::engine::{HostConfig, HostEngine};
use crate::events::{HostEvent, LogLevel};
use crate::stream::StreamSettings;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// De quanto em quanto tempo o terminal confere teclado e eventos.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// O resumo da transmissão chega a cada 1 s; o terminal mostra só a cada 5 s.
const STATS_PRINT_INTERVAL: Duration = Duration::from_secs(5);

/// Pergunta que o Host está respondendo no terminal (depois do comando `share`).
#[derive(Clone, Copy)]
enum Prompt {
    /// Nenhuma pergunta aberta: o que for digitado é um comando.
    None,
    /// Esperando a resolução (720 ou 1080).
    Resolution,
    /// Esperando o FPS (15 ou 30), com a resolução já escolhida.
    Fps { height: u32 },
}

/// O que pode chegar ao terminal vindo de fora do laço principal.
enum Input {
    Line(String),
    CtrlC,
}

/// Roda o Host no terminal até o Ctrl+C.
pub fn run() -> io::Result<()> {
    // Pergunta a senha antes de abrir a sala (Enter vazio = sem senha)
    let password = ask_room_password()?;

    let engine = HostEngine::start(HostConfig {
        password,
        max_clients: None, // sem limite (vai virar uma opção na GUI)
        room_name: None,
    })?;

    let (input_tx, input_rx) = mpsc::channel::<Input>();
    spawn_stdin_reader(input_tx.clone());

    // Ctrl+C vira um comando de encerrar
    let ctrl_c_tx = input_tx.clone();
    engine.runtime_handle().spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = ctrl_c_tx.send(Input::CtrlC);
    });

    let mut printer = EventPrinter::new();
    printer.drain(&engine);
    println!("[host] Comandos: list | kick <id> | share | stop | help");

    let mut prompt = Prompt::None;

    loop {
        printer.drain(&engine);

        match input_rx.recv_timeout(POLL_INTERVAL) {
            Ok(Input::Line(line)) => handle_input(&line, &mut prompt, &engine),
            Ok(Input::CtrlC) => break,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("[host] Encerrando... avisando os clients.");
    for event in engine.stop() {
        printer.print(&event, None);
    }

    Ok(())
}

/// Pergunta a senha da sala sem mostrar o que é digitado.
/// Vazio (ou só espaços) = sala sem senha. A senha fica só na memória.
fn ask_room_password() -> io::Result<Option<String>> {
    let typed = rpassword::prompt_password("[host] Senha da sala (Enter para sem senha): ")?;

    if typed.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(typed))
    }
}

/// Lê o que o Host digita no terminal (numa thread própria, porque ler o teclado bloqueia).
fn spawn_stdin_reader(tx: Sender<Input>) {
    std::thread::spawn(move || {
        for line in io::stdin().lines() {
            let Ok(line) = line else { break };
            if tx.send(Input::Line(line)).is_err() {
                break;
            }
        }
    });
}

/// Mostra os eventos do motor no formato de texto de sempre.
struct EventPrinter {
    last_stats: Instant,
}

impl EventPrinter {
    fn new() -> Self {
        Self {
            last_stats: Instant::now(),
        }
    }

    /// Mostra todos os eventos que estão esperando.
    fn drain(&mut self, engine: &HostEngine) {
        while let Some(event) = engine.try_next_event() {
            self.print(&event, Some(engine));
        }
    }

    /// `engine` = None quando a sala já foi fechada (aí não dá para mostrar a lista de quem está conectado).
    fn print(&mut self, event: &HostEvent, engine: Option<&HostEngine>) {
        match event {
            HostEvent::Started {
                address,
                has_password,
                max_clients,
                local_addresses,
            } => {
                match max_clients {
                    Some(max) => println!("[host] Escutando em {address} (maximo {max} clients)"),
                    None => println!("[host] Escutando em {address} (sem limite de clients)"),
                }
                if *has_password {
                    println!("[host] Sala protegida por senha.");
                } else {
                    println!("[host] Sala sem senha.");
                }
                if !local_addresses.is_empty() {
                    println!("[host] Use um destes enderecos no Client:");
                    for local in local_addresses {
                        println!("[host]   {}   ({})", local.ip, local.interface);
                    }
                }
                println!("[host] Ctrl+C para encerrar.");
            }

            HostEvent::Stopped => {}

            HostEvent::UserJoined { id, name, addr } => {
                println!("[host] {name} (#{id}) entrou ({addr})");
                print_roster_if_open(engine);
            }

            HostEvent::UserLeft {
                id,
                name,
                addr,
                reason,
            } => {
                println!("[host] {name} (#{id}) saiu ({addr}): {reason}");
                print_roster_if_open(engine);
            }

            HostEvent::UserKicked { id, name, addr } => {
                println!("[host] {name} (#{id}) foi removido ({addr})");
                print_roster_if_open(engine);
            }

            HostEvent::UserRejected { name, reason, .. } => {
                println!("[host] {name} recusado: {reason}");
            }

            HostEvent::PasswordFailed {
                name,
                addr,
                attempt,
                max_attempts,
            } => {
                println!(
                    "[host] {name} ({addr}) errou a senha (tentativa {attempt}/{max_attempts})"
                );
            }

            // O terminal não mostra o ping (a GUI vai mostrar)
            HostEvent::Ping { .. } => {}

            HostEvent::StreamStarted {
                source_width,
                source_height,
                width,
                height,
                fps,
            } => {
                println!(
                    "[host] Transmitindo a tela ({source_width}x{source_height} -> {width}x{height}, {fps} FPS). Digite 'stop' para parar."
                );
                self.last_stats = Instant::now();
            }

            HostEvent::StreamStats {
                packets_per_sec,
                mbps,
                clients,
                skipped_frames,
            } => {
                if self.last_stats.elapsed() >= STATS_PRINT_INTERVAL {
                    println!(
                        "[host] Transmitindo: {packets_per_sec:.1} pacotes/s, {mbps:.2} Mbps (por client), {clients} client(s), {skipped_frames} frame(s) pulado(s)"
                    );
                    self.last_stats = Instant::now();
                }
            }

            HostEvent::StreamStopped { error } => {
                if let Some(erro) = error {
                    println!("[host] A transmissao parou: {erro}");
                    println!("[host] Digite 'share' para tentar de novo.");
                }
            }

            HostEvent::Log { level, message } => match level {
                LogLevel::Debug => println!("[diag] {message}"),
                LogLevel::Info | LogLevel::Warn => println!("[host] {message}"),
            },
        }
    }
}

fn print_roster_if_open(engine: Option<&HostEngine>) {
    if let Some(engine) = engine {
        print_roster(engine);
    }
}

/// Mostra quem está conectado agora.
fn print_roster(engine: &HostEngine) {
    let users = engine.users();
    if users.is_empty() {
        println!("[host] Ninguem conectado.");
        return;
    }

    println!("[host] Conectados agora ({}):", users.len());
    for user in users {
        println!(
            "[host]   #{} {} - {} - ha {}s",
            user.id, user.name, user.addr, user.connected_secs
        );
    }
}

/// Lê o número de segundos de um comando (padrão: 5).
fn parse_seconds(text: Option<&str>) -> u64 {
    text.and_then(|t| t.parse::<u64>().ok())
        .filter(|&s| s > 0)
        .unwrap_or(5)
}

/// Resolução digitada: 720 ou 1080 (aceita também "720p" e "1080p").
fn parse_resolution(text: &str) -> Option<u32> {
    match text.trim().to_lowercase().trim_end_matches('p') {
        "720" => Some(720),
        "1080" => Some(1080),
        _ => None,
    }
}

/// FPS digitado: 15 ou 30.
fn parse_fps(text: &str) -> Option<u32> {
    match text.trim() {
        "15" => Some(15),
        "30" => Some(30),
        _ => None,
    }
}

/// Decide o que fazer com uma linha digitada: resposta de uma pergunta aberta ou comando.
fn handle_input(line: &str, prompt: &mut Prompt, engine: &HostEngine) {
    match *prompt {
        Prompt::None => handle_command(line, prompt, engine),

        // Resposta inválida (outro número, Enter vazio...): não faz nada e continua esperando
        Prompt::Resolution => {
            if let Some(height) = parse_resolution(line) {
                println!("[host] Resolucao escolhida: {height}p");
                println!("[host] FPS da transmissao (digite 15 ou 30):");
                *prompt = Prompt::Fps { height };
            }
        }

        Prompt::Fps { height } => {
            if let Some(fps) = parse_fps(line) {
                *prompt = Prompt::None;
                start_sharing(engine, StreamSettings { height, fps });
            }
        }
    }
}

/// Executa um comando digitado pelo Host.
fn handle_command(line: &str, prompt: &mut Prompt, engine: &HostEngine) {
    let mut parts = line.split_whitespace();

    match parts.next() {
        Some("list") => print_roster(engine),

        Some("kick") => {
            // Aceita "kick 3" e "kick #3"
            let id = parts
                .next()
                .and_then(|text| text.trim_start_matches('#').parse::<u64>().ok());

            match id {
                Some(id) => match engine.kick(id) {
                    Ok(name) => println!("[host] Removendo {name} (#{id})..."),
                    Err(motivo) => println!("[host] {motivo} Use 'list'."),
                },
                None => println!("[host] Uso: kick <id>   (veja os ids com 'list')"),
            }
        }

        Some("share") => begin_share(engine, prompt),

        Some("stop") => stop_sharing(engine),

        Some("capture") => start_capture_test(parse_seconds(parts.next())),

        Some("encode") => start_encode_test(parse_seconds(parts.next())),

        Some("hwencode") => start_hw_encode_test(parse_seconds(parts.next())),

        Some("help") => print_help(),

        Some(outro) => println!("[host] Comando desconhecido: '{outro}'. Digite 'help'."),

        None => {}
    }
}

/// Começa o `share`: se a tela ainda não está sendo compartilhada, abre a pergunta da resolução.
fn begin_share(engine: &HostEngine, prompt: &mut Prompt) {
    if engine.is_streaming() {
        println!("[host] A tela ja esta sendo compartilhada. Use 'stop' para parar.");
        return;
    }

    println!("[host] Resolucao da transmissao (digite 720 ou 1080):");
    *prompt = Prompt::Resolution;
}

/// Começa a transmitir a tela para os Clients, com as opções escolhidas.
fn start_sharing(engine: &HostEngine, settings: StreamSettings) {
    match engine.start_stream(settings) {
        Ok(()) => println!(
            "[host] Compartilhando a tela em {}p a {} FPS (a GPU so e usada quando ha clients conectados)...",
            settings.height, settings.fps
        ),
        Err(motivo) => println!("[host] {motivo} Use 'stop' para parar."),
    }
}

/// Para a transmissão da tela.
fn stop_sharing(engine: &HostEngine) {
    if engine.stop_stream() {
        println!("[host] Compartilhamento parado.");
    } else {
        println!("[host] A tela nao esta sendo compartilhada.");
    }
}

/// Teste temporário da captura de tela.
/// Roda numa thread própria para não travar o Host enquanto captura.
fn start_capture_test(seconds: u64) {
    println!("[host] Capturando a tela por {seconds}s...");

    std::thread::spawn(move || {
        if let Err(erro) = crate::capture::test_capture(seconds) {
            println!("[host] Falha na captura: {erro}");
        }
    });
}

/// Teste temporário da compressão por software (captura + H.264 na CPU).
fn start_encode_test(seconds: u64) {
    println!("[host] Capturando e comprimindo (CPU) a tela por {seconds}s...");

    std::thread::spawn(move || {
        if let Err(erro) = crate::encoder::test_encode(seconds) {
            println!("[host] Falha na compressao: {erro}");
        }
    });
}

/// Teste temporário da compressão por hardware (captura + H.264 na GPU).
fn start_hw_encode_test(seconds: u64) {
    println!("[host] Capturando e comprimindo (GPU) a tela por {seconds}s...");

    std::thread::spawn(move || {
        if let Err(erro) = crate::hw_encoder::test_hw_encode(seconds) {
            println!("[host] Falha na compressao por GPU: {erro}");
        }
    });
}

fn print_help() {
    println!("[host] Comandos:");
    println!("[host]   list              mostra quem esta conectado");
    println!("[host]   kick <id>         remove o client com esse id (veja o id em 'list')");
    println!("[host]   share             comeca a transmitir a tela (pergunta resolucao e FPS)");
    println!("[host]   stop              para a transmissao da tela");
    println!("[host]   capture [segs]    teste de captura da tela (padrao: 5 segundos)");
    println!("[host]   encode [segs]     teste de captura + compressao H.264 na CPU");
    println!("[host]   hwencode [segs]   teste de captura + compressao H.264 na GPU");
    println!("[host]   help              mostra esta ajuda");
}
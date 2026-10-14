//! LANShare: um único executável que roda como Host ou como Client.
//!
//! Uso:
//!   lanshare                 pergunta se você quer abrir como Host ou Client
//!   lanshare host            abre uma sala
//!   lanshare client          procura salas na rede
//!   lanshare client <ip>     conecta direto no IP
//!
//! (Provisório: quando a GUI entrar, a escolha Host/Client passa a ser feita na janela.)

use std::io::{self, Write};

enum Mode {
    Host,
    Client,
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);

    let mode = match args.next() {
        Some(text) => match parse_mode(&text) {
            Some(mode) => mode,
            None => {
                print_usage();
                return Ok(());
            }
        },
        // Sem argumento (por exemplo, duplo clique no .exe): pergunta
        None => ask_mode()?,
    };

    match mode {
        // O Host tem o seu próprio runtime do tokio dentro do motor
        Mode::Host => lanshare_host::run_cli(),
        Mode::Client => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(lanshare_client::run(args.next()))
        }
    }
}

/// "host" ou "h" = Host; "client" ou "c" = Client.
fn parse_mode(text: &str) -> Option<Mode> {
    match text.trim().to_lowercase().as_str() {
        "host" | "h" => Some(Mode::Host),
        "client" | "c" => Some(Mode::Client),
        _ => None,
    }
}

/// Pergunta no terminal se este computador será Host ou Client.
fn ask_mode() -> io::Result<Mode> {
    loop {
        print!("[lanshare] Abrir como (h)ost ou (c)lient? ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "entrada fechada",
            ));
        }

        if let Some(mode) = parse_mode(&input) {
            return Ok(mode);
        }
        println!("[lanshare] Digite 'h' para Host ou 'c' para Client.");
    }
}

fn print_usage() {
    println!("Uso:");
    println!("  lanshare                 pergunta se abre como Host ou Client");
    println!("  lanshare host            abre uma sala");
    println!("  lanshare client          procura salas na rede");
    println!("  lanshare client <ip>     conecta direto no IP");
}
//! Biblioteca do Host: abre a sala, aceita vários Clients ao mesmo tempo, transmite a tela do Host
//! e coordena as transmissões de todos (quem transmite, ingressos para assistir).
//!
//! - `HostEngine` (engine.rs) é o motor: recebe comandos e emite eventos (events.rs).
//! - `run_cli()` (cli.rs) é o terminal que usa o motor (`lanshare host`).
//! - streams.rs guarda quem está transmitindo na sala e onde assistir.
//! - A captura, a compressão e o envio moram no crate lanshare-stream.
//! - A interface gráfica usa o mesmo motor pela FFI.

mod cli;
mod discovery;
mod engine;
mod events;
mod kick;
mod registry;
mod stream_bridge;
mod streams;

pub use engine::{HostConfig, HostEngine};
pub use events::{HostEvent, LocalAddress, LogLevel, UserEntry};
pub use lanshare_stream::{app_windows, audio_sessions, monitors};
pub use lanshare_stream::{CaptureTarget, StreamSettings, PREVIEW_MAX_HEIGHT, PREVIEW_MAX_WIDTH};

/// Roda o Host no terminal até o Ctrl+C (chamado pelo executável único).
pub fn run_cli() -> std::io::Result<()> {
    cli::run()
}
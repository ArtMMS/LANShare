//! Biblioteca do Host: abre a sala, aceita vários Clients ao mesmo tempo e transmite a tela (e o som).
//!
//! - `HostEngine` (engine.rs) é o motor: recebe comandos e emite eventos (events.rs).
//! - `run_cli()` (cli.rs) é o terminal que usa o motor (`lanshare host`).
//! - A captura, a compressão e o envio moram no crate lanshare-stream (stream_bridge.rs faz a ligação).
//! - A interface gráfica usa o mesmo motor pela FFI.

mod cli;
mod discovery;
mod engine;
mod events;
mod kick;
mod registry;
mod stream_bridge;

// Atalhos com os caminhos antigos (crate::stream, crate::capture...): o engine.rs e o cli.rs
// continuam iguais. Saem quando o motor for refeito (Etapa 3 da v2.0).
use lanshare_stream::{audio_capture, capture, encoder, hw_encoder, preview, stream};

pub use engine::{HostConfig, HostEngine};
pub use events::{HostEvent, LocalAddress, LogLevel, UserEntry};
pub use lanshare_stream::{app_windows, audio_sessions, monitors};
pub use lanshare_stream::{CaptureTarget, StreamSettings, PREVIEW_MAX_HEIGHT, PREVIEW_MAX_WIDTH};

/// Roda o Host no terminal até o Ctrl+C (chamado pelo executável único).
pub fn run_cli() -> std::io::Result<()> {
    cli::run()
}
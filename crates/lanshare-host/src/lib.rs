//! Biblioteca do Host: abre a sala, aceita vários Clients ao mesmo tempo e transmite a tela (e o som).
//!
//! - `HostEngine` (engine.rs) é o motor: recebe comandos e emite eventos (events.rs).
//! - `run_cli()` (cli.rs) é o terminal que usa o motor (`lanshare host`).
//! - A interface gráfica usa o mesmo motor pela FFI.

pub mod app_windows;
mod audio_capture;
pub mod audio_sessions;
mod audio_stream;
mod capture;
mod cli;
mod discovery;
mod encoder;
mod engine;
mod events;
mod hw_encoder;
mod kick;
pub mod monitors;
mod preview;
mod registry;
mod stream;

pub use capture::CaptureTarget;
pub use engine::{HostConfig, HostEngine};
pub use events::{HostEvent, LocalAddress, LogLevel, UserEntry};
pub use preview::{PREVIEW_MAX_HEIGHT, PREVIEW_MAX_WIDTH};
pub use stream::StreamSettings;

/// Roda o Host no terminal até o Ctrl+C (chamado pelo executável único).
pub fn run_cli() -> std::io::Result<()> {
    cli::run()
}
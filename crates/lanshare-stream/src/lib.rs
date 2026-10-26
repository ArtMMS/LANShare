//! lanshare-stream: transmissão da tela e do som (captura -> compressão -> envio).
//! Usado por quem transmite: hoje o Host; na v2.0, qualquer pessoa da sala.
//!
//! Não sabe nada de sala nem de conexões: entrega os pacotes a um `StreamSink` (sink.rs)
//! e avisa o que acontece por `StreamEvents` (events.rs).

pub mod app_windows;
pub mod audio_capture;
mod audio_mixer;
pub mod audio_sessions;
mod audio_stream;
pub mod capture;
pub mod encoder;
mod events;
pub mod hw_encoder;
pub mod monitors;
pub mod preview;
mod sink;
pub mod stream;

pub use capture::CaptureTarget;
pub use events::{StreamEvent, StreamEvents};
pub use preview::{PreviewSlot, PREVIEW_MAX_HEIGHT, PREVIEW_MAX_WIDTH};
pub use sink::StreamSink;
pub use stream::{StreamHandle, StreamSettings};
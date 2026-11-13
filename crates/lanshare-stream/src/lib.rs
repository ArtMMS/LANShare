//! lanshare-stream: transmissão da tela e do som (captura -> compressão -> envio).
//! A webcam também é capturada e transmitida aqui (camera.rs e camera_stream.rs).
//! Usado por quem transmite: o Host ou qualquer Client.
//!
//! A transmissão entrega os pacotes a um `StreamSink` (sink.rs) e avisa o que acontece
//! por `StreamEvents` (events.rs). O `StreamServer` (server.rs) é o sink da v2.0:
//! a porta TCP onde quem tem ingresso assiste.

pub mod app_windows;
pub mod audio_capture;
mod audio_mixer;
pub mod audio_sessions;
mod audio_stream;
pub mod camera;
mod camera_stream;
pub mod capture;
pub mod encoder;
mod events;
pub mod hw_encoder;
pub mod monitors;
pub mod preview;
mod server;
mod sink;
pub mod stream;

pub use camera::{CameraCapture, CameraDevice, CameraMode, CameraRequest, CameraTestPreview};
pub use camera_stream::{CameraBroadcast, CameraSettings, CAMERA_TILE_FPS, CAMERA_TILE_HEIGHT};
pub use capture::CaptureTarget;
pub use events::{StreamEvent, StreamEvents};
pub use preview::{PreviewSlot, PREVIEW_MAX_HEIGHT, PREVIEW_MAX_WIDTH};
pub use server::StreamServer;
pub use sink::{Congestion, StreamSink};
pub use stream::{StreamHandle, StreamSettings};

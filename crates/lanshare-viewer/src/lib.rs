//! lanshare-viewer: assistir uma transmissão (H.264 -> imagem, Opus -> alto-falante).
//! Usado por quem assiste: o Host e os Clients.
//!
//! - watcher.rs: qual transmissão estou assistindo (uma por vez) e a conexão com quem transmite;
//! - session.rs: UMA conexão de transmissão (ingresso, vídeo e som);
//! - video.rs: decodifica o vídeo numa thread própria e guarda a imagem mais recente;
//! - audio_player.rs: toca o som na saída escolhida e é o relógio da sincronia (sync_clock.rs);
//! - audio_devices.rs: lista as saídas de áudio (para as Configurações).

use serde_json::Value;
use std::sync::mpsc::Sender;

pub mod audio_devices;
pub mod audio_player;
pub mod decoder;
mod session;
pub mod sync_clock;
pub mod video;
mod watcher;

pub use watcher::Watcher;

/// Canal por onde avisamos a interface (cada evento é um JSON com "type").
pub type EventTx = Sender<Value>;
//! Eventos da transmissão: começou, resumo, parou e avisos.
//! Quem usa decide o que fazer com eles (o Host transforma em HostEvent, no stream_bridge.rs).

use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// O primeiro frame foi codificado: a transmissão está no ar.
    Started {
        source_width: u32,
        source_height: u32,
        width: u32,
        height: u32,
        fps: u32,
    },

    /// Resumo (a cada ~1 segundo, enquanto alguém assiste).
    Stats {
        packets_per_sec: f64,
        /// Megabits por segundo enviados a CADA espectador.
        mbps: f64,
        viewers: usize,
        skipped_frames: u32,
    },

    /// A transmissão terminou. `error` = None quando foi parada normalmente.
    Stopped { error: Option<String> },

    Debug(String),
    Info(String),
    Warn(String),
}

/// Quem recebe os eventos. Pode ser clonado e levado para outras threads.
#[derive(Clone)]
pub struct StreamEvents {
    emit: Arc<dyn Fn(StreamEvent) + Send + Sync>,
}

impl StreamEvents {
    pub fn new(emit: impl Fn(StreamEvent) + Send + Sync + 'static) -> Self {
        Self { emit: Arc::new(emit) }
    }

    pub fn emit(&self, event: StreamEvent) {
        (self.emit)(event);
    }

    pub fn debug(&self, message: impl Into<String>) {
        self.emit(StreamEvent::Debug(message.into()));
    }

    pub fn info(&self, message: impl Into<String>) {
        self.emit(StreamEvent::Info(message.into()));
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.emit(StreamEvent::Warn(message.into()));
    }
}
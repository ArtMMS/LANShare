//! A transmissão do próprio Client (v2.0): captura -> StreamServer -> quem tem ingresso.
//! A sala fica sabendo pelo StreamAnnounce (ao começar) e pelo StreamEnded (ao parar).
//!
//! Eventos (JSON): own_stream_started, own_stream_stats, own_stream_stopped
//! (com nomes próprios para não confundir com a transmissão que o Client assiste).

use lanshare_core::i18n::tr;
use lanshare_core::protocol::Message;
use lanshare_stream::{
    CaptureTarget, PreviewSlot, StreamEvent, StreamEvents, StreamHandle, StreamServer,
    StreamSettings,
};
use lanshare_viewer::EventTx;
use serde_json::json;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

struct Running {
    handle: StreamHandle,
    server: StreamServer,
    /// Caixa de saída da sala (para o StreamEnded).
    room: UnboundedSender<Message>,
}

/// A minha transmissão. Clonar NÃO copia: todos os clones veem a mesma.
#[derive(Clone, Default)]
pub(crate) struct OwnStream {
    inner: Arc<Mutex<Option<Running>>>,
}

impl OwnStream {
    /// Abre a porta, começa a capturar e avisa a sala (StreamAnnounce).
    pub fn start(
        &self,
        runtime: &Handle,
        events: &EventTx,
        room: UnboundedSender<Message>,
        preview: PreviewSlot,
        settings: StreamSettings,
        target: CaptureTarget,
        audio_enabled: bool,
    ) -> Result<(), String> {
        let mut slot = self.lock();
        if slot.as_ref().is_some_and(|r| r.handle.is_running()) {
            return Err(tr(
                "A tela já está sendo compartilhada.",
                "The screen is already being shared.",
                "La pantalla ya se está compartiendo.",
            )
            .to_string());
        }

        // Uma transmissão anterior que caiu sozinha: a thread dela já terminou (não precisa esperar)
        if let Some(old) = slot.take() {
            old.server.stop();
        }

        let server = StreamServer::start(runtime, self.stream_events(events.clone())).map_err(|erro| {
            format!(
                "{}: {erro}",
                tr(
                    "Não foi possível abrir a porta da transmissão",
                    "Could not open the stream port",
                    "No se pudo abrir el puerto de la transmisión"
                )
            )
        })?;
        let port = server.port();

        let handle = StreamHandle::start_on(
            server.clone(),
            settings,
            self.stream_events(events.clone()),
            preview,
            target,
            audio_enabled,
        );

        if room.send(Message::StreamAnnounce { port }).is_err() {
            // Já saiu da sala: desfaz tudo
            server.stop();
            drop(slot);
            handle.stop();
            return Err(not_in_room());
        }

        *slot = Some(Running {
            handle,
            server,
            room,
        });
        Ok(())
    }

    /// Para a transmissão e avisa a sala (StreamEnded). false = não estava transmitindo.
    pub fn stop(&self) -> bool {
        // Tira da caixa antes de esperar a thread terminar (para não segurar o lock)
        let running = self.lock().take();
        let Some(running) = running else {
            return false;
        };

        running.server.stop();
        let _ = running.room.send(Message::StreamEnded);
        running.handle.stop();
        true
    }

    pub fn is_running(&self) -> bool {
        self.lock().as_ref().is_some_and(|r| r.handle.is_running())
    }

    pub fn switch_source(&self, target: CaptureTarget) -> Result<(), String> {
        let slot = self.lock();
        match slot.as_ref() {
            Some(running) if running.handle.is_running() => running.handle.switch_source(target),
            _ => Err(tr(
                "Não há transmissão em andamento.",
                "There is no stream in progress.",
                "No hay ninguna transmisión en curso.",
            )
            .to_string()),
        }
    }

    pub fn set_audio_enabled(&self, enabled: bool) {
        if let Some(running) = self.lock().as_ref().filter(|r| r.handle.is_running()) {
            running.handle.set_audio_enabled(enabled);
        }
    }

    /// O Host autorizou alguém a me assistir (ViewerAllowed).
    pub fn allow_viewer(&self, viewer_id: u64, ticket: String) {
        if let Some(running) = self.lock().as_ref() {
            running.server.allow_viewer(viewer_id, ticket);
        }
    }

    /// O Host mandou derrubar alguém (ViewerRevoked).
    pub fn revoke_viewer(&self, viewer_id: u64) {
        if let Some(running) = self.lock().as_ref() {
            running.server.revoke_viewer(viewer_id);
        }
    }

    /// A captura caiu sozinha (ex.: a janela foi fechada): fecha a porta e avisa a sala.
    /// Chamado de dentro da própria thread da transmissão: por isso não espera ela terminar.
    fn died(&self) {
        let running = self.lock().take();
        if let Some(running) = running {
            running.server.stop();
            let _ = running.room.send(Message::StreamEnded);
            // O `handle` é descartado aqui sem esperar: a thread dele é esta, que já está terminando
        }
    }

    /// Eventos da transmissão -> JSON para a interface.
    fn stream_events(&self, events: EventTx) -> StreamEvents {
        let slot = self.clone();
        StreamEvents::new(move |event| {
            let message = match event {
                StreamEvent::Started {
                    source_width,
                    source_height,
                    width,
                    height,
                    fps,
                } => json!({
                    "type": "own_stream_started",
                    "source_width": source_width,
                    "source_height": source_height,
                    "width": width,
                    "height": height,
                    "fps": fps,
                }),
                StreamEvent::Stats {
                    packets_per_sec,
                    mbps,
                    viewers,
                    skipped_frames,
                } => json!({
                    "type": "own_stream_stats",
                    "packets_per_sec": packets_per_sec,
                    "mbps": mbps,
                    "viewers": viewers,
                    "skipped_frames": skipped_frames,
                }),
                StreamEvent::Stopped { error } => {
                    if error.is_some() {
                        slot.died();
                    }
                    json!({ "type": "own_stream_stopped", "error": error })
                }
                StreamEvent::Debug(_) => return,
                StreamEvent::Info(message) => {
                    json!({ "type": "log", "level": "info", "message": message })
                }
                StreamEvent::Warn(message) => {
                    json!({ "type": "log", "level": "warn", "message": message })
                }
            };
            let _ = events.send(message);
        })
    }

    fn lock(&self) -> MutexGuard<'_, Option<Running>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn not_in_room() -> String {
    tr(
        "Você não está numa sala.",
        "You are not in a room.",
        "No estás en ninguna sala.",
    )
    .to_string()
}
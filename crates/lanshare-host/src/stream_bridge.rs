//! Liga os eventos do lanshare-stream aos eventos do Host
//! (o mesmo JSON de antes para a interface, mais o bitrate adaptativo).
//! A câmera tem eventos próprios (camera_started, camera_stats, camera_stopped).

use crate::events::{EventSender, HostEvent};
use lanshare_stream::{StreamEvent, StreamEvents};

impl From<EventSender> for StreamEvents {
    fn from(events: EventSender) -> Self {
        StreamEvents::new(move |event| match event {
            StreamEvent::Started {
                source_width,
                source_height,
                width,
                height,
                fps,
            } => events.emit(HostEvent::StreamStarted {
                source_width,
                source_height,
                width,
                height,
                fps,
            }),
            StreamEvent::Stats {
                packets_per_sec,
                mbps,
                viewers,
                skipped_frames,
                bitrate_mbps,
                unstable,
            } => events.emit(HostEvent::StreamStats {
                packets_per_sec,
                mbps,
                clients: viewers,
                skipped_frames,
                bitrate_mbps,
                unstable,
            }),
            StreamEvent::Stopped { error } => events.emit(HostEvent::StreamStopped { error }),
            StreamEvent::Debug(message) => events.debug(message),
            StreamEvent::Info(message) => events.info(message),
            StreamEvent::Warn(message) => events.warn(message),
        })
    }
}

/// Eventos da câmera do Host -> eventos próprios (para não confundir com a transmissão da tela).
pub(crate) fn camera_events(events: EventSender) -> StreamEvents {
    StreamEvents::new(move |event| match event {
        StreamEvent::Started {
            width, height, fps, ..
        } => events.emit(HostEvent::CameraStarted { width, height, fps }),
        StreamEvent::Stats {
            mbps,
            viewers,
            bitrate_mbps,
            unstable,
            ..
        } => events.emit(HostEvent::CameraStats {
            mbps,
            viewers,
            bitrate_mbps,
            unstable,
        }),
        StreamEvent::Stopped { error } => events.emit(HostEvent::CameraStopped { error }),
        StreamEvent::Debug(message) => events.debug(message),
        StreamEvent::Info(message) => events.info(message),
        StreamEvent::Warn(message) => events.warn(message),
    })
}

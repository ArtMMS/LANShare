//! Liga os eventos do lanshare-stream aos eventos do Host
//! (o mesmo JSON de antes para a interface, mais o bitrate adaptativo).

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
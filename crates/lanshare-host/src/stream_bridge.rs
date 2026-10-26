//! Liga o lanshare-stream à sala do Host:
//! - a sala (Registry) recebe os pacotes da transmissão e repassa aos Clients;
//! - os eventos da transmissão viram eventos do Host (o mesmo JSON de antes para a interface).

use crate::events::{EventSender, HostEvent};
use crate::registry::Registry;
use lanshare_stream::{StreamEvent, StreamEvents, StreamSink};
use std::sync::Arc;

impl StreamSink for Registry {
    fn has_viewers(&self) -> bool {
        !self.is_empty()
    }

    fn viewer_count(&self) -> usize {
        self.list().len()
    }

    fn take_keyframe_request(&self) -> bool {
        Registry::take_keyframe_request(self)
    }

    fn send_video(&self, keyframe: bool, timestamp_us: u64, data: Arc<Vec<u8>>) {
        self.broadcast_video(keyframe, timestamp_us, data);
    }

    fn send_audio(&self, timestamp_us: u64, data: Arc<Vec<u8>>) {
        self.broadcast_audio(timestamp_us, data);
    }
}

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
            } => events.emit(HostEvent::StreamStats {
                packets_per_sec,
                mbps,
                clients: viewers,
                skipped_frames,
            }),
            StreamEvent::Stopped { error } => events.emit(HostEvent::StreamStopped { error }),
            StreamEvent::Debug(message) => events.debug(message),
            StreamEvent::Info(message) => events.info(message),
            StreamEvent::Warn(message) => events.warn(message),
        })
    }
}
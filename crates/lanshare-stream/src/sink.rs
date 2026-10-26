//! Para onde vão os pacotes da transmissão: quem está assistindo.
//! Hoje é a sala do Host (o Registry); na v2.0 vira a lista de espectadores de cada transmissor.

use std::sync::Arc;

pub trait StreamSink: Send + Sync + 'static {
    /// Tem alguém assistindo? Sem ninguém, a transmissão não gasta a GPU nem o Opus.
    fn has_viewers(&self) -> bool;

    /// Quantos estão assistindo (vai no resumo da transmissão).
    fn viewer_count(&self) -> usize;

    /// Alguém precisa de um keyframe novo? (Lê e zera o pedido.)
    fn take_keyframe_request(&self) -> bool;

    /// Um pacote H.264 para todos que assistem.
    fn send_video(&self, keyframe: bool, timestamp_us: u64, data: Arc<Vec<u8>>);

    /// Um pacote Opus (20 ms de som) para todos que assistem.
    fn send_audio(&self, timestamp_us: u64, data: Arc<Vec<u8>>);
}
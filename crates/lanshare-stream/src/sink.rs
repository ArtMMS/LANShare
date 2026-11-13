//! Para onde vão os pacotes da transmissão: quem está assistindo.
//! Hoje é o StreamServer (a porta de quem transmite, com a lista de espectadores).

use std::sync::Arc;

/// O quanto os espectadores estão ficando para trás, desde a última pergunta.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Congestion {
    /// Maior fila de vídeo (em imagens) que algum espectador chegou a ter.
    pub max_queued: usize,
    /// Algum espectador encheu a fila e perdeu imagens (vai esperar um keyframe).
    pub overflowed: bool,
}

pub trait StreamSink: Send + Sync + 'static {
    /// Tem alguém assistindo? Sem ninguém, a transmissão não gasta a GPU nem o Opus.
    fn has_viewers(&self) -> bool;

    /// Quantos estão assistindo (vai no resumo da transmissão).
    fn viewer_count(&self) -> usize;

    /// Alguém precisa de um keyframe novo? (Lê e zera o pedido.)
    fn take_keyframe_request(&self) -> bool;

    /// Como estão as filas dos espectadores desde a última pergunta (lê e zera).
    /// É o que o bitrate adaptativo usa para saber se alguém não está dando conta.
    fn take_congestion(&self) -> Congestion;

    /// Um pacote H.264 para todos que assistem.
    fn send_video(&self, keyframe: bool, timestamp_us: u64, data: Arc<Vec<u8>>);

    /// Um pacote Opus (20 ms de som) para todos que assistem.
    fn send_audio(&self, timestamp_us: u64, data: Arc<Vec<u8>>);
}
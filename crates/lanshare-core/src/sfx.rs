//! Efeitos sonoros (soundboard), compartilhado entre o Host e o Client.
//!
//! - Quem toca: a interface entrega o som já decodificado (PCM float, 48 kHz, estéreo)
//!   e `encode` comprime em pacotes Opus de 20 ms para ir pela rede (um som de 5 s fica com ~60 KB).
//! - Quem recebe: o som chega ainda comprimido e fica guardado no `SfxInbox` até a interface
//!   buscar com `take`, que descomprime (fora da conexão: a rede nunca espera a decodificação).

use crate::i18n::tr;
use crate::protocol::{MAX_SFX_PACKETS, MAX_SFX_PACKET_BYTES};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

/// Amostras (intercaladas) de um pacote Opus de 20 ms.
const FRAME_SAMPLES: usize = 960 * CHANNELS;

/// Maior som aceito, em amostras intercaladas (5,2 s). A interface (C#) reserva este tamanho: mantenha igual.
pub const MAX_SAMPLES: usize = MAX_SFX_PACKETS * FRAME_SAMPLES;

/// Tamanho máximo do nome (em caracteres).
pub const MAX_NAME_LEN: usize = 32;

/// Qualidade dos efeitos: 96 kbps em estéreo é praticamente igual ao original para sons curtos.
const BITRATE_BPS: i32 = 96_000;

/// Maior pacote Opus decodificado: 120 ms de estéreo.
const MAX_DECODED_SAMPLES: usize = 5760 * CHANNELS;

/// Quantos sons recebidos podem esperar a interface buscar. Passou disso, o mais antigo é descartado
/// (o terminal, por exemplo, nunca busca: assim a memória não cresce).
const INBOX_SIZE: usize = 16;

/// Comprime um som (PCM float intercalado, 48 kHz, estéreo) em pacotes Opus de 20 ms.
/// O último pacote é completado com silêncio.
pub fn encode(pcm: &[f32]) -> Result<Vec<Vec<u8>>, String> {
    if pcm.len() < CHANNELS {
        return Err(tr(
            "O som está vazio.",
            "The sound is empty.",
            "El sonido está vacío.",
        )
        .to_string());
    }
    if pcm.len() > MAX_SAMPLES {
        return Err(tr(
            "O som é longo demais (máximo de 5 segundos).",
            "The sound is too long (5 seconds max).",
            "El sonido es demasiado largo (máximo 5 segundos).",
        )
        .to_string());
    }

    let fail = |erro: opus::Error| {
        format!(
            "{}: {erro}",
            tr(
                "Não foi possível comprimir o som",
                "Could not compress the sound",
                "No se pudo comprimir el sonido"
            )
        )
    };

    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio).map_err(fail)?;
    encoder.set_bitrate(Bitrate::Bits(BITRATE_BPS)).map_err(fail)?;

    let mut frame = vec![0i16; FRAME_SAMPLES];
    let mut packet = vec![0u8; MAX_SFX_PACKET_BYTES];
    let mut packets = Vec::with_capacity(pcm.len().div_ceil(FRAME_SAMPLES));

    for chunk in pcm.chunks(FRAME_SAMPLES) {
        // float -> 16 bits; o que faltar no último pacote vira silêncio
        let samples = chunk.iter().copied().chain(std::iter::repeat(0.0));
        for (dst, src) in frame.iter_mut().zip(samples) {
            *dst = (src.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        }

        let len = encoder.encode(&frame, &mut packet).map_err(fail)?;
        packets.push(packet[..len].to_vec());
    }

    Ok(packets)
}

/// Descomprime os pacotes Opus de um som em PCM float intercalado (48 kHz, estéreo).
pub fn decode(packets: &[Vec<u8>]) -> Result<Vec<f32>, String> {
    let fail = |erro: opus::Error| {
        format!(
            "{}: {erro}",
            tr(
                "Não foi possível abrir o som recebido",
                "Could not open the received sound",
                "No se pudo abrir el sonido recibido"
            )
        )
    };

    let mut decoder = Decoder::new(SAMPLE_RATE, Channels::Stereo).map_err(fail)?;
    let mut pcm = vec![0i16; MAX_DECODED_SAMPLES];
    let mut out = Vec::with_capacity(packets.len() * FRAME_SAMPLES);

    for packet in packets {
        let frames = decoder.decode(packet, &mut pcm, false).map_err(fail)?;
        out.extend(pcm[..frames * CHANNELS].iter().map(|s| *s as f32 / 32768.0));
        if out.len() >= MAX_SAMPLES {
            out.truncate(MAX_SAMPLES);
            break;
        }
    }

    Ok(out)
}

/// Limpa o nome que veio da rede: sem caracteres de controle, no máximo MAX_NAME_LEN caracteres.
pub fn clean_name(raw: &str) -> String {
    let name: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect();
    name.trim().to_string()
}

#[derive(Default)]
struct InboxData {
    next_id: u64,
    /// (id, pacotes Opus), do mais antigo para o mais novo.
    items: VecDeque<(u64, Arc<Vec<Vec<u8>>>)>,
}

/// Sons recebidos esperando a interface buscar (pelo id que veio no evento "sfx_played").
/// Clonar NÃO copia: todos os clones veem a mesma caixa.
#[derive(Clone, Default)]
pub struct SfxInbox {
    inner: Arc<Mutex<InboxData>>,
}

impl SfxInbox {
    /// Guarda um som recebido e devolve o id dele (começa em 1).
    pub fn put(&self, packets: Arc<Vec<Vec<u8>>>) -> u64 {
        let mut data = self.lock();
        data.next_id += 1;
        let id = data.next_id;
        data.items.push_back((id, packets));
        while data.items.len() > INBOX_SIZE {
            data.items.pop_front();
        }
        id
    }

    /// Tira o som da caixa e descomprime. None = não existe (já buscado ou descartado) ou inválido.
    pub fn take(&self, id: u64) -> Option<Vec<f32>> {
        let packets = {
            let mut data = self.lock();
            let index = data.items.iter().position(|(item, _)| *item == id)?;
            data.items.remove(index)?.1
        };

        // Fora do lock: descomprimir leva alguns milissegundos
        decode(&packets).ok()
    }

    fn lock(&self) -> MutexGuard<'_, InboxData> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 s de um tom de 440 Hz, estéreo.
    fn tone(seconds: f32) -> Vec<f32> {
        let frames = (SAMPLE_RATE as f32 * seconds) as usize;
        (0..frames)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn comprime_e_descomprime_com_a_mesma_duracao() {
        let pcm = tone(1.0);
        let packets = encode(&pcm).unwrap();
        assert_eq!(packets.len(), 50); // 1 s / 20 ms

        let back = decode(&packets).unwrap();
        assert_eq!(back.len(), 50 * FRAME_SAMPLES);
    }

    #[test]
    fn som_longo_demais_e_recusado() {
        assert!(encode(&tone(5.3)).is_err());
        assert!(encode(&[]).is_err());
    }

    #[test]
    fn caixa_entrega_uma_vez_e_descarta_os_antigos() {
        let inbox = SfxInbox::default();
        let packets = Arc::new(encode(&tone(0.1)).unwrap());

        let first = inbox.put(packets.clone());
        assert!(inbox.take(first).is_some());
        assert!(inbox.take(first).is_none()); // já foi buscado

        let old = inbox.put(packets.clone());
        for _ in 0..INBOX_SIZE {
            inbox.put(packets.clone());
        }
        assert!(inbox.take(old).is_none()); // descartado por ser o mais antigo
    }

    #[test]
    fn nome_vindo_da_rede_e_limpo() {
        assert_eq!(clean_name("  Quack\n🦆  "), "Quack🦆");
        assert_eq!(clean_name(&"a".repeat(100)).chars().count(), MAX_NAME_LEN);
    }
}

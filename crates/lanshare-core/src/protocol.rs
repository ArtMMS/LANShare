//! Protocolo de comunicação entre Host e Client.
//!
//! Cada mensagem trafega assim pela rede:
//!
//!   [ 4 bytes: tamanho N ] [ N bytes: mensagem em JSON ]
//!
//! O tamanho na frente é necessário porque o TCP é um fluxo contínuo de
//! bytes e não avisa onde termina uma mensagem e começa a outra.

use serde::{Deserialize, Serialize};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u16 = 1;

/// Porta padrão onde o Host escuta.
pub const DEFAULT_PORT: u16 = 47800;

/// Tamanho máximo aceito para uma mensagem (1 MiB).
/// Protege contra alguém mandar um tamanho absurdo e travar o programa.
const MAX_FRAME_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    Hello {
        protocol_version: u16,
        device_name: String,
    },

    HelloAck {
        accepted: bool,
        reason: Option<String>,
    },
}


pub async fn write_message<W>(writer: &mut W, message: &Message) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(message)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if payload.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mensagem grande demais",
        ));
    }

    writer.write_u32(payload.len() as u32).await?;

    writer.write_all(&payload).await?;

    writer.flush().await?;

    Ok(())
}

/// Lê UMA mensagem completa da conexão.
///
/// Se o outro lado fechar a conexão, esta função devolve um erro do tipo
/// UnexpectedEof. É assim que, mais para frente, vamos detectar desconexões.
pub async fn read_message<R>(reader: &mut R) -> io::Result<Message>
where
    R: AsyncRead + Unpin,
{
    let length = reader.read_u32().await? as usize;

    if length > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mensagem grande demais",
        ));
    }

    let mut buffer = vec![0u8; length];
    reader.read_exact(&mut buffer).await?;
    serde_json::from_slice(&buffer)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// Este bloco só é compilado quando rodamos "cargo test".
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hello_chega_igual() {
        let (mut a, mut b) = tokio::io::duplex(1024);

        let enviada = Message::Hello {
            protocol_version: PROTOCOL_VERSION,
            device_name: "pc-de-teste".to_string(),
        };

        write_message(&mut a, &enviada).await.unwrap();
        let recebida = read_message(&mut b).await.unwrap();

        assert_eq!(enviada, recebida);
    }
}
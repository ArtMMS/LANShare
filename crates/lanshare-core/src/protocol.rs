//! Protocolo Host <-> Client.
//! Formato na rede: [4 bytes: tamanho N][N bytes: mensagem em JSON]

use serde::{Deserialize, Serialize};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Aumente sempre que o formato das mensagens mudar de forma incompatível.
pub const PROTOCOL_VERSION: u16 = 3;
pub const DEFAULT_PORT: u16 = 47800;

/// De quanto em quanto tempo enviamos um Ping.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Sem receber nada por este tempo, o outro lado é considerado desconectado.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(6);

/// Tamanho máximo de uma mensagem (1 MiB).
const MAX_FRAME_SIZE: usize = 1024 * 1024;

/// Um usuário conectado, como os Clients o enxergam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    /// Client -> Host: primeira mensagem.
    Hello {
        protocol_version: u16,
        device_name: String,
    },

    /// Host -> Client: aceito. Diz quem o Client é e quem está na sala (inclui ele mesmo).
    Welcome {
        client_id: u64,
        display_name: String,
        users: Vec<UserInfo>,
    },

    /// Host -> Client: recusado, com o motivo.
    Rejected { reason: String },

    /// Host -> Clients: alguém entrou na sala.
    UserJoined { user: UserInfo },

    /// Host -> Clients: alguém saiu da sala.
    UserLeft { user: UserInfo },

    /// "Você está aí?" O outro lado responde com um Pong do mesmo id.
    Ping { id: u64 },

    /// Resposta ao Ping.
    Pong { id: u64 },

    /// Aviso de saída limpa.
    Bye,
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

/// Lê UMA mensagem. Se o outro lado fechar a conexão, devolve UnexpectedEof.
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

#[cfg(test)]
mod tests {
    use super::*;

    // Manda cada tipo de mensagem por um "cano" em memória e confere que chega igual.
    #[tokio::test]
    async fn todas_as_mensagens_chegam_iguais() {
        let (mut a, mut b) = tokio::io::duplex(4096);

        let ana = UserInfo {
            id: 1,
            name: "Ana".to_string(),
        };
        let beto = UserInfo {
            id: 2,
            name: "Beto".to_string(),
        };

        let mensagens = [
            Message::Hello {
                protocol_version: PROTOCOL_VERSION,
                device_name: "pc-de-teste".to_string(),
            },
            Message::Welcome {
                client_id: 2,
                display_name: "Beto".to_string(),
                users: vec![ana.clone(), beto.clone()],
            },
            Message::Rejected {
                reason: "sala cheia".to_string(),
            },
            Message::UserJoined { user: beto.clone() },
            Message::UserLeft { user: ana },
            Message::Ping { id: 7 },
            Message::Pong { id: 7 },
            Message::Bye,
        ];

        for enviada in mensagens {
            write_message(&mut a, &enviada).await.unwrap();
            let recebida = read_message(&mut b).await.unwrap();
            assert_eq!(enviada, recebida);
        }
    }
}
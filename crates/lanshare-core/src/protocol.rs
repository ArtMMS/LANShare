//! Protocolo Host <-> Client.
//! Formato na rede: [4 bytes: tamanho N][1 byte: tipo][N-1 bytes: conteúdo]
//!   tipo 0 = mensagem em JSON
//!   tipo 1 = pacote de vídeo, em binário: [1 byte: keyframe][8 bytes: timestamp em µs][bytes H.264]
//!   tipo 2 = pacote de áudio, em binário: [8 bytes: timestamp em µs][bytes Opus]

use serde::{Deserialize, Serialize};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Aumente sempre que o formato das mensagens mudar de forma incompatível.
pub const PROTOCOL_VERSION: u16 = 9;
pub const DEFAULT_PORT: u16 = 47800;

/// De quanto em quanto tempo enviamos um Ping.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Sem receber nada por este tempo, o outro lado é considerado desconectado.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(6);

/// Id usado para o Host nos avatares (os Clients começam em 1).
pub const HOST_USER_ID: u64 = 0;

/// Tamanho máximo de um avatar na rede (PNG em base64).
pub const MAX_AVATAR_LEN: usize = 256 * 1024;

/// Motivos de recusa (campo `code` do Rejected). O Client mostra o texto no idioma dele;
/// o `reason` em texto continua indo para Clients de versões antigas.
pub const REJECT_PROTOCOL_MISMATCH: &str = "protocol_mismatch";
pub const REJECT_ROOM_FULL: &str = "room_full";
pub const REJECT_WRONG_PASSWORD: &str = "wrong_password";
pub const REJECT_PASSWORD_TIMEOUT: &str = "password_timeout";

/// Tipos de mensagem na rede.
const KIND_JSON: u8 = 0;
const KIND_VIDEO: u8 = 1;
const KIND_AUDIO: u8 = 2;

/// Tamanho máximo de uma mensagem JSON (1 MiB).
const MAX_JSON_SIZE: usize = 1024 * 1024;

/// Tamanho máximo dos bytes de vídeo de um pacote (8 MiB).
const MAX_VIDEO_SIZE: usize = 8 * 1024 * 1024;

/// Cabeçalho do vídeo: 1 byte (keyframe) + 8 bytes (timestamp).
const VIDEO_HEADER_SIZE: usize = 9;

/// Tamanho máximo dos bytes de áudio de um pacote (um pacote Opus tem no máximo ~1,3 KB).
const MAX_AUDIO_SIZE: usize = 64 * 1024;

/// Cabeçalho do áudio: 8 bytes (timestamp).
const AUDIO_HEADER_SIZE: usize = 8;

/// Um usuário conectado, como os Clients o enxergam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: u64,
    pub name: String,
}

/// Um pacote de vídeo H.264 (Host -> Clients). Não passa pelo JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    /// true = quadro completo (dá para começar a decodificar a partir dele).
    pub keyframe: bool,
    /// Momento do envio, em microssegundos desde o início da transmissão.
    pub timestamp_us: u64,
    /// Bytes H.264 (compartilhados entre os Clients, sem copiar).
    pub data: Arc<Vec<u8>>,
    queue_ticket: QueueTicket,
}

impl VideoFrame {
    pub fn new(keyframe: bool, timestamp_us: u64, data: Arc<Vec<u8>>) -> Self {
        Self {
            keyframe,
            timestamp_us,
            data,
            queue_ticket: QueueTicket(None),
        }
    }

    /// Usado pelo Host: ligado a um contador de "frames esperando na fila de um Client".
    /// O contador diminui sozinho quando este frame deixa de existir
    /// (foi enviado pela rede ou descartado).
    pub fn with_queue_counter(mut self, counter: Arc<AtomicUsize>) -> Self {
        self.queue_ticket = QueueTicket(Some(counter));
        self
    }
}

/// Um pacote de áudio Opus (Host -> Clients): 20 ms de som estéreo a 48 kHz. Não passa pelo JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// Quando o som tocou no Host, em microssegundos desde o início da transmissão
    /// (o mesmo relógio do vídeo).
    pub timestamp_us: u64,
    /// Bytes Opus (compartilhados entre os Clients, sem copiar).
    pub data: Arc<Vec<u8>>,
}

/// Diminui o contador da fila quando o frame some. Copiar o frame NÃO copia o ticket.
#[derive(Debug, Default)]
struct QueueTicket(Option<Arc<AtomicUsize>>);

impl Clone for QueueTicket {
    fn clone(&self) -> Self {
        QueueTicket(None)
    }
}

impl PartialEq for QueueTicket {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Drop for QueueTicket {
    fn drop(&mut self) {
        if let Some(counter) = &self.0 {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    /// Client -> Host: primeira mensagem.
    Hello {
        protocol_version: u16,
        device_name: String,
    },

    /// Host -> Client: esta sala tem senha; envie um Password.
    PasswordRequired,

    /// Client -> Host: a senha digitada pelo usuário.
    Password { password: String },

    /// Host -> Client: aceito. Diz quem o Client é, quem está na sala (inclui ele mesmo)
    /// e o nome de usuário do Host.
    Welcome {
        client_id: u64,
        display_name: String,
        users: Vec<UserInfo>,
        /// Vazio quando o Host é de uma versão que ainda não envia o nome.
        #[serde(default)]
        host_name: String,
    },

    /// Host -> Client: recusado, com o motivo (texto) e o código do motivo (REJECT_*).
    Rejected {
        reason: String,
        /// Vazio quando o Host é de uma versão que ainda não envia o código.
        #[serde(default)]
        code: String,
    },

    /// Host -> Clients: alguém entrou na sala.
    UserJoined { user: UserInfo },

    /// Host -> Clients: alguém saiu da sala.
    UserLeft { user: UserInfo },

    /// Host -> Client: você foi removido da sala (a conexão vai ser encerrada).
    Kicked { reason: String },

    /// Host -> Clients: alguém foi removido da sala pelo Host.
    UserKicked { user: UserInfo },

    /// Client -> Host: o meu avatar (PNG em base64). Vazio = sem avatar (usar o padrão).
    Avatar { png_base64: String },

    /// Host -> Clients: o avatar de alguém (id 0 = o Host). Vazio = sem avatar.
    UserAvatar { id: u64, png_base64: String },

    /// Host -> Clients: o Host parou de transmitir a tela (a conexão continua aberta).
    StreamStopped,

    /// "Você está aí?" O outro lado responde com um Pong do mesmo id.
    Ping { id: u64 },

    /// Resposta ao Ping.
    Pong { id: u64 },

    /// Aviso de saída limpa.
    Bye,

    /// Host -> Clients: um pacote de vídeo da tela.
    /// Não vai em JSON: tem formato binário próprio (ver write_message).
    #[serde(skip)]
    Video(VideoFrame),

    /// Host -> Clients: um pacote de áudio do som do PC.
    /// Não vai em JSON: tem formato binário próprio (ver write_message).
    #[serde(skip)]
    Audio(AudioFrame),
}

pub async fn write_message<W>(writer: &mut W, message: &Message) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match message {
        Message::Video(frame) => return write_video(writer, frame).await,
        Message::Audio(frame) => return write_audio(writer, frame).await,
        _ => {}
    }

    let payload = serde_json::to_vec(message)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if payload.len() > MAX_JSON_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mensagem grande demais",
        ));
    }

    // Tudo num buffer só: uma escrita na rede em vez de várias pequenas
    let mut buffer = Vec::with_capacity(5 + payload.len());
    buffer.extend_from_slice(&((payload.len() + 1) as u32).to_be_bytes());
    buffer.push(KIND_JSON);
    buffer.extend_from_slice(&payload);

    writer.write_all(&buffer).await?;
    writer.flush().await?;

    Ok(())
}

async fn write_video<W>(writer: &mut W, frame: &VideoFrame) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let data = frame.data.as_slice();

    if data.len() > MAX_VIDEO_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pacote de video grande demais",
        ));
    }

    let body_len = 1 + VIDEO_HEADER_SIZE + data.len();

    let mut buffer = Vec::with_capacity(4 + body_len);
    buffer.extend_from_slice(&(body_len as u32).to_be_bytes());
    buffer.push(KIND_VIDEO);
    buffer.push(frame.keyframe as u8);
    buffer.extend_from_slice(&frame.timestamp_us.to_be_bytes());
    buffer.extend_from_slice(data);

    writer.write_all(&buffer).await?;
    writer.flush().await?;

    Ok(())
}

async fn write_audio<W>(writer: &mut W, frame: &AudioFrame) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let data = frame.data.as_slice();

    if data.len() > MAX_AUDIO_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pacote de audio grande demais",
        ));
    }

    let body_len = 1 + AUDIO_HEADER_SIZE + data.len();

    let mut buffer = Vec::with_capacity(4 + body_len);
    buffer.extend_from_slice(&(body_len as u32).to_be_bytes());
    buffer.push(KIND_AUDIO);
    buffer.extend_from_slice(&frame.timestamp_us.to_be_bytes());
    buffer.extend_from_slice(data);

    writer.write_all(&buffer).await?;
    writer.flush().await?;

    Ok(())
}

/// Lê UMA mensagem. Se o outro lado fechar a conexão, devolve UnexpectedEof.
pub async fn read_message<R>(reader: &mut R) -> io::Result<Message>
where
    R: AsyncRead + Unpin,
{
    let length = reader.read_u32().await? as usize;

    if length == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mensagem vazia"));
    }

    let kind = reader.read_u8().await?;
    let body_len = length - 1;

    match kind {
        KIND_JSON => {
            if body_len > MAX_JSON_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "mensagem grande demais",
                ));
            }

            let mut buffer = vec![0u8; body_len];
            reader.read_exact(&mut buffer).await?;

            serde_json::from_slice(&buffer)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        KIND_VIDEO => {
            if body_len < VIDEO_HEADER_SIZE || body_len - VIDEO_HEADER_SIZE > MAX_VIDEO_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "tamanho de video invalido",
                ));
            }

            let keyframe = reader.read_u8().await? != 0;
            let timestamp_us = reader.read_u64().await?;

            let mut data = vec![0u8; body_len - VIDEO_HEADER_SIZE];
            reader.read_exact(&mut data).await?;

            Ok(Message::Video(VideoFrame::new(
                keyframe,
                timestamp_us,
                Arc::new(data),
            )))
        }

        KIND_AUDIO => {
            if body_len < AUDIO_HEADER_SIZE || body_len - AUDIO_HEADER_SIZE > MAX_AUDIO_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "tamanho de audio invalido",
                ));
            }

            let timestamp_us = reader.read_u64().await?;

            let mut data = vec![0u8; body_len - AUDIO_HEADER_SIZE];
            reader.read_exact(&mut data).await?;

            Ok(Message::Audio(AudioFrame {
                timestamp_us,
                data: Arc::new(data),
            }))
        }

        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "tipo de mensagem desconhecido",
        )),
    }
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
            Message::PasswordRequired,
            Message::Password {
                password: "segredo".to_string(),
            },
            Message::Welcome {
                client_id: 2,
                display_name: "Beto".to_string(),
                users: vec![ana.clone(), beto.clone()],
                host_name: "Carla".to_string(),
            },
            Message::Rejected {
                reason: "sala cheia".to_string(),
                code: REJECT_ROOM_FULL.to_string(),
            },
            Message::UserJoined { user: beto.clone() },
            Message::UserLeft { user: ana },
            Message::Kicked {
                reason: "Voce foi removido pelo Host".to_string(),
            },
            Message::UserKicked { user: beto },
            Message::Avatar {
                png_base64: "iVBORw0KGgo=".to_string(),
            },
            Message::UserAvatar {
                id: HOST_USER_ID,
                png_base64: String::new(),
            },
            Message::StreamStopped,
            Message::Ping { id: 7 },
            Message::Pong { id: 7 },
            Message::Bye,
            Message::Video(VideoFrame::new(true, 123_456, Arc::new(vec![0, 0, 0, 1, 0x65, 9, 8, 7]))),
            Message::Video(VideoFrame::new(false, 987_654, Arc::new(vec![1, 2, 3]))),
            Message::Audio(AudioFrame {
                timestamp_us: 20_000,
                data: Arc::new(vec![0xFC, 0xFF, 0xFE]),
            }),
        ];

        for enviada in mensagens {
            write_message(&mut a, &enviada).await.unwrap();
            let recebida = read_message(&mut b).await.unwrap();
            assert_eq!(enviada, recebida);
        }
    }

    // Um Welcome de um Host antigo (sem host_name) ainda é entendido.
    #[test]
    fn welcome_sem_host_name_ainda_funciona() {
        let json = r#"{"Welcome":{"client_id":1,"display_name":"Ana","users":[]}}"#;
        let message: Message = serde_json::from_str(json).unwrap();
        match message {
            Message::Welcome { host_name, .. } => assert!(host_name.is_empty()),
            outra => panic!("mensagem inesperada: {outra:?}"),
        }
    }

    // Um Rejected de um Host antigo (sem code) ainda é entendido.
    #[test]
    fn rejected_sem_code_ainda_funciona() {
        let json = r#"{"Rejected":{"reason":"sala cheia"}}"#;
        let message: Message = serde_json::from_str(json).unwrap();
        match message {
            Message::Rejected { reason, code } => {
                assert_eq!(reason, "sala cheia");
                assert!(code.is_empty());
            }
            outra => panic!("mensagem inesperada: {outra:?}"),
        }
    }

    // O contador da fila diminui quando o frame some.
    #[test]
    fn contador_da_fila_diminui_quando_o_frame_some() {
        let contador = Arc::new(AtomicUsize::new(1));
        let frame = VideoFrame::new(false, 0, Arc::new(vec![1])).with_queue_counter(contador.clone());

        // Copiar o frame não mexe no contador
        let copia = frame.clone();
        drop(copia);
        assert_eq!(contador.load(Ordering::Relaxed), 1);

        drop(frame);
        assert_eq!(contador.load(Ordering::Relaxed), 0);
    }
}

//! Descoberta de salas na LAN (UDP broadcast).
//! O Client pergunta "tem sala aí?" e cada Host responde com os dados da sala.

use serde::{Deserialize, Serialize};

/// Porta UDP onde o Host escuta os pedidos de descoberta.
pub const DISCOVERY_PORT: u16 = 47801;

/// Dados de uma sala, como aparecem na lista do Client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomInfo {
    /// Identifica a sala (o Client usa para não listar a mesma sala duas vezes).
    pub room_id: u64,
    pub name: String,
    /// Porta TCP para conectar.
    pub port: u16,
    pub protocol_version: u16,
    pub has_password: bool,
    pub users: u32,
    pub max_users: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiscoveryPacket {
    /// Client -> rede: "quem tem sala aberta?"
    Request,

    /// Host -> Client: "eu tenho esta sala".
    Response { room: RoomInfo },
}

pub fn encode(packet: &DiscoveryPacket) -> Vec<u8> {
    serde_json::to_vec(packet).unwrap_or_default()
}

/// Devolve None se o pacote não for nosso (lixo ou outro programa na mesma porta).
pub fn decode(bytes: &[u8]) -> Option<DiscoveryPacket> {
    serde_json::from_slice(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pacotes_de_descoberta_chegam_iguais() {
        let room = RoomInfo {
            room_id: 42,
            name: "PC-DE-TESTE".to_string(),
            port: 47800,
            protocol_version: 4,
            has_password: true,
            users: 2,
            max_users: Some(8),
        };

        for packet in [DiscoveryPacket::Request, DiscoveryPacket::Response { room }] {
            assert_eq!(decode(&encode(&packet)), Some(packet));
        }
    }
}
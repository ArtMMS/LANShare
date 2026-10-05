//! Responde aos pedidos de descoberta: "tem sala aberta aqui?"

use crate::registry::Registry;
use lanshare_core::discovery::{decode, encode, DiscoveryPacket, RoomInfo, DISCOVERY_PORT};
use lanshare_core::protocol::{DEFAULT_PORT, PROTOCOL_VERSION};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;

/// Fica ouvindo pedidos na porta UDP e responde com os dados atuais da sala.
pub async fn run_responder(registry: Registry, has_password: bool, max_clients: Option<usize>) {
    let socket = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await {
        Ok(socket) => socket,
        Err(erro) => {
            println!("[host] Descoberta automatica desativada (porta UDP {DISCOVERY_PORT}): {erro}");
            return;
        }
    };

    // Identifica esta sala; muda a cada vez que o Host abre
    let room_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Host".to_string());

    println!("[host] Descoberta automatica ativa (UDP {DISCOVERY_PORT})");

    let mut buffer = [0u8; 1024];
    loop {
        let (length, from) = match socket.recv_from(&mut buffer).await {
            Ok(received) => received,
            Err(_) => {
                // No Windows um erro de rede antigo pode aparecer aqui; só segue em frente
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };

        if decode(&buffer[..length]) != Some(DiscoveryPacket::Request) {
            continue;
        }

        let room = RoomInfo {
            room_id,
            name: name.clone(),
            port: DEFAULT_PORT,
            protocol_version: PROTOCOL_VERSION,
            has_password,
            users: registry.users().len() as u32,
            max_users: max_clients.map(|max| max as u32),
        };

        let _ = socket
            .send_to(&encode(&DiscoveryPacket::Response { room }), from)
            .await;
    }
}
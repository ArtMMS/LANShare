//! Procura salas na rede: manda um pedido em broadcast e junta as respostas.

use lanshare_core::discovery::{decode, encode, DiscoveryPacket, RoomInfo, DISCOVERY_PORT};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Instant};

/// Uma sala encontrada e o IP de onde veio a resposta.
pub struct FoundRoom {
    pub ip: IpAddr,
    pub info: RoomInfo,
}

impl FoundRoom {
    /// Endereço pronto para conectar, ex.: "192.168.0.10:47800".
    pub fn address(&self) -> String {
        format!("{}:{}", self.ip, self.info.port)
    }
}

/// Procura salas por cerca de 2 segundos (dois pedidos, para não depender de um só pacote).
pub async fn scan() -> io::Result<Vec<FoundRoom>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;

    let targets = broadcast_targets();
    let request = encode(&DiscoveryPacket::Request);
    let mut found: HashMap<u64, FoundRoom> = HashMap::new();

    for _ in 0..2 {
        for target in &targets {
            let _ = socket.send_to(&request, target).await;
        }
        listen(&socket, Duration::from_secs(1), &mut found).await;
    }

    let mut rooms: Vec<FoundRoom> = found.into_values().collect();
    rooms.sort_by(|a, b| a.info.name.cmp(&b.info.name).then(a.ip.cmp(&b.ip)));
    Ok(rooms)
}

/// Escuta respostas durante `duration`, guardando cada sala uma vez só.
async fn listen(socket: &UdpSocket, duration: Duration, found: &mut HashMap<u64, FoundRoom>) {
    let deadline = Instant::now() + duration;
    let mut buffer = [0u8; 1024];

    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }

        match timeout(left, socket.recv_from(&mut buffer)).await {
            Ok(Ok((length, from))) => {
                let Some(DiscoveryPacket::Response { room }) = decode(&buffer[..length]) else {
                    continue;
                };

                let id = room.room_id;
                let candidate = FoundRoom {
                    ip: from.ip(),
                    info: room,
                };

                // A mesma sala pode responder por mais de um IP; prefere o que não é 127.0.0.1
                let keep_existing = match found.get(&id) {
                    Some(existing) => !existing.ip.is_loopback() || candidate.ip.is_loopback(),
                    None => false,
                };
                if !keep_existing {
                    found.insert(id, candidate);
                }
            }
            Ok(Err(_)) => continue, // erro de rede avulso; o prazo final encerra o loop
            Err(_) => break,        // acabou o tempo
        }
    }
}

/// Endereço de broadcast de cada placa de rede (inclui a do RadminVPN) + o geral.
fn broadcast_targets() -> Vec<SocketAddr> {
    let mut targets = vec![SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT))];

    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if interface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = &interface.addr {
                if v4.ip.is_link_local() {
                    continue;
                }
                // broadcast = IP com todos os bits "livres" da máscara ligados
                let broadcast = Ipv4Addr::from(u32::from(v4.ip) | !u32::from(v4.netmask));
                let target = SocketAddr::from((broadcast, DISCOVERY_PORT));
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
        }
    }

    targets
}
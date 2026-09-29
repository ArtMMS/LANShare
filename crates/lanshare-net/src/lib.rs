//! lanshare-net: cuida da conexão entre Host e Client depois do handshake.

pub mod connection;

// Atalhos, para usar "lanshare_net::run_connection" direto.
pub use connection::{run_connection, ConnectionEvent, DisconnectReason};
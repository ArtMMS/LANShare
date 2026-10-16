//! Eventos do Host: tudo o que acontece e que a interface (terminal ou janela) precisa saber.
//!
//! O motor (engine.rs) não imprime nada: ele só emite eventos, e quem usa decide como mostrá-los.
//! Os eventos viram JSON (campo "type" + dados) para a interface em C# ler pela FFI.

use serde::Serialize;
use std::sync::mpsc::Sender;

/// Um endereço IP deste computador (para o usuário saber o que digitar no Client).
#[derive(Debug, Clone, Serialize)]
pub struct LocalAddress {
    pub ip: String,
    pub interface: String,
}

/// Um Client conectado, como a interface o enxerga.
#[derive(Debug, Clone, Serialize)]
pub struct UserEntry {
    pub id: u64,
    pub name: String,
    pub addr: String,
    /// Há quantos segundos está conectado.
    pub connected_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Só para diagnóstico (a interface normal ignora).
    Debug,
    Info,
    Warn,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostEvent {
    /// A sala foi aberta.
    Started {
        address: String,
        has_password: bool,
        max_clients: Option<usize>,
        local_addresses: Vec<LocalAddress>,
    },

    /// A sala foi fechada.
    Stopped,

    /// Um Client entrou na sala.
    UserJoined { id: u64, name: String, addr: String },

    /// Um Client saiu da sala (Bye, queda de rede, encerramento do Host...).
    UserLeft {
        id: u64,
        name: String,
        addr: String,
        reason: String,
    },

    /// Um Client foi removido pelo Host.
    UserKicked { id: u64, name: String, addr: String },

    /// Um Client mandou (ou trocou) o avatar. Vazio = sem avatar (usar o padrão).
    UserAvatar { id: u64, png_base64: String },

    /// Alguém tentou entrar e foi recusado (senha errada, sala cheia, versão incompatível...).
    UserRejected {
        name: String,
        addr: String,
        reason: String,
    },

    /// Alguém errou a senha (ainda pode tentar de novo).
    PasswordFailed {
        name: String,
        addr: String,
        attempt: u32,
        max_attempts: u32,
    },

    /// Ping medido até um Client, em milissegundos.
    Ping { id: u64, rtt_ms: u64 },

    /// A transmissão da tela começou.
    StreamStarted {
        source_width: u32,
        source_height: u32,
        width: u32,
        height: u32,
        fps: u32,
    },

    /// Resumo da transmissão (a cada ~1 segundo, enquanto há Clients).
    StreamStats {
        packets_per_sec: f64,
        /// Megabits por segundo enviados a CADA client.
        mbps: f64,
        clients: usize,
        skipped_frames: u32,
    },

    /// A transmissão terminou. `error` = None quando foi o Host que parou.
    StreamStopped { error: Option<String> },

    /// Mensagem de texto (avisos e diagnóstico).
    Log { level: LogLevel, message: String },
}

/// Quem emite eventos. Pode ser clonado e levado para outras threads e tarefas.
#[derive(Clone)]
pub struct EventSender {
    tx: Sender<HostEvent>,
}

impl EventSender {
    pub fn new(tx: Sender<HostEvent>) -> Self {
        Self { tx }
    }

    /// Erro = ninguém está mais ouvindo os eventos; não tem problema.
    pub fn emit(&self, event: HostEvent) {
        let _ = self.tx.send(event);
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.emit(HostEvent::Log {
            level,
            message: message.into(),
        });
    }

    pub fn debug(&self, message: impl Into<String>) {
        self.log(LogLevel::Debug, message);
    }

    pub fn info(&self, message: impl Into<String>) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogLevel::Warn, message);
    }
}
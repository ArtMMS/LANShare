//! lanshare-ffi: ponte entre o motor Rust do LANShare e a interface em C# (Avalonia).
//!
//! Só funções em C ABI (`extern "C"`). A interface chama cada uma via P/Invoke.
//! Textos que voltam do Rust (JSON) são alocados aqui e DEVEM ser liberados
//! pelo C# com `lanshare_free_string`.

use lanshare_host::{HostConfig, HostEngine, StreamSettings};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

/// O Host em execução (um por processo).
static HOST: Mutex<Option<HostEngine>> = Mutex::new(None);

/// Versão do motor Rust, como texto C (UTF-8 terminado em `\0`).
/// O ponteiro aponta para memória estática: quem chama NÃO deve liberá-lo.
#[no_mangle]
pub extern "C" fn lanshare_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Libera um texto devolvido por qualquer função `lanshare_*` que retorne `char*` alocado.
#[no_mangle]
pub extern "C" fn lanshare_free_string(text: *mut c_char) {
    if !text.is_null() {
        // Devolve a posse do texto ao Rust, que o libera
        drop(unsafe { CString::from_raw(text) });
    }
}

/// Abre a sala. `config_json`: {"password":"..","max_clients":4,"room_name":".."} (tudo opcional).
/// Retorna JSON: {"ok":true} ou {"ok":false,"error":"..."}.
#[no_mangle]
pub extern "C" fn lanshare_host_start(config_json: *const c_char) -> *mut c_char {
    let mut host = HOST.lock().unwrap();
    if host.is_some() {
        return reply_err("a sala já está aberta");
    }

    let text = if config_json.is_null() {
        "{}".to_string()
    } else {
        unsafe { CStr::from_ptr(config_json) }
            .to_string_lossy()
            .into_owned()
    };
    let config: HostConfig = match serde_json::from_str(&text) {
        Ok(config) => config,
        Err(error) => return reply_err(&format!("configuração inválida: {error}")),
    };

    match HostEngine::start(config) {
        Ok(engine) => {
            *host = Some(engine);
            reply_ok()
        }
        Err(error) => reply_err(&format!("não foi possível abrir a sala: {error}")),
    }
}

/// Próximo evento do Host como JSON (ex.: {"type":"user_joined",...}), ou NULL se não há nenhum.
#[no_mangle]
pub extern "C" fn lanshare_host_poll_event() -> *mut c_char {
    let host = HOST.lock().unwrap();
    match host.as_ref().and_then(|engine| engine.try_next_event()) {
        Some(event) => to_c(serde_json::to_string(&event).unwrap_or_default()),
        None => std::ptr::null_mut(),
    }
}

/// Lista de Clients conectados, como JSON: [{"id":1,"name":"..","addr":"..","connected_secs":5}].
#[no_mangle]
pub extern "C" fn lanshare_host_users() -> *mut c_char {
    let host = HOST.lock().unwrap();
    let users = host.as_ref().map(|engine| engine.users()).unwrap_or_default();
    to_c(serde_json::to_string(&users).unwrap_or_else(|_| "[]".into()))
}

/// Expulsa um Client pelo id. Retorna {"ok":true,"name":".."} ou {"ok":false,"error":".."}.
#[no_mangle]
pub extern "C" fn lanshare_host_kick(id: u64) -> *mut c_char {
    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err("a sala não está aberta"),
        Some(engine) => match engine.kick(id) {
            Ok(name) => to_c(serde_json::json!({ "ok": true, "name": name }).to_string()),
            Err(error) => reply_err(&error),
        },
    }
}

/// Começa a transmitir a tela. `height`: 720 ou 1080; `fps`: 15 ou 30.
#[no_mangle]
pub extern "C" fn lanshare_host_start_stream(height: u32, fps: u32) -> *mut c_char {
    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err("a sala não está aberta"),
        Some(engine) => match engine.start_stream(StreamSettings { height, fps }) {
            Ok(()) => reply_ok(),
            Err(error) => reply_err(&error),
        },
    }
}

/// Para a transmissão. Retorna 1 se havia uma transmissão, 0 se não.
#[no_mangle]
pub extern "C" fn lanshare_host_stop_stream() -> i32 {
    let host = HOST.lock().unwrap();
    host.as_ref().map(|e| e.stop_stream()).unwrap_or(false) as i32
}

/// 1 se está transmitindo, 0 se não.
#[no_mangle]
pub extern "C" fn lanshare_host_is_streaming() -> i32 {
    let host = HOST.lock().unwrap();
    host.as_ref().map(|e| e.is_streaming()).unwrap_or(false) as i32
}

/// Fecha a sala (para a transmissão e desconecta todos).
#[no_mangle]
pub extern "C" fn lanshare_host_stop() {
    let engine = HOST.lock().unwrap().take();
    if let Some(engine) = engine {
        let _ = engine.stop();
    }
}

fn to_c(text: String) -> *mut c_char {
    // JSON nunca tem byte nulo, então o CString não falha
    CString::new(text).unwrap_or_default().into_raw()
}

fn reply_ok() -> *mut c_char {
    to_c(r#"{"ok":true}"#.to_string())
}

fn reply_err(message: &str) -> *mut c_char {
    to_c(serde_json::json!({ "ok": false, "error": message }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_a_valid_c_string() {
        let text = unsafe { CStr::from_ptr(lanshare_version()) };
        assert_eq!(text.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn errors_are_json() {
        let ptr = reply_err("falhou \"x\"");
        let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        lanshare_free_string(ptr);
        assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
    }
}
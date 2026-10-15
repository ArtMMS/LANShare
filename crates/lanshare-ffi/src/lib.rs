//! lanshare-ffi: ponte entre o motor Rust do LANShare e a interface em C# (Avalonia).
//!
//! Só funções em C ABI (`extern "C"`). A interface chama cada uma via P/Invoke.
//! Textos que voltam do Rust (JSON) são alocados aqui e DEVEM ser liberados
//! pelo C# com `lanshare_free_string`.

use lanshare_client::ClientEngine;
use lanshare_core::settings::{self, Settings};
use lanshare_host::monitors;
use lanshare_host::{HostConfig, HostEngine, StreamSettings};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Mutex, MutexGuard};

/// O Host em execução (um por processo).
static HOST: Mutex<Option<HostEngine>> = Mutex::new(None);

/// O Client em execução (um por processo).
static CLIENT: Mutex<Option<ClientEngine>> = Mutex::new(None);

/// Tamanho máximo do nome de usuário (em caracteres).
const MAX_USERNAME_LEN: usize = 24;

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

// ───────────────────────────── Configurações ─────────────────────────────

/// Nome de usuário salvo: {"username":"Ana"} ou {"username":null} (primeira abertura).
#[no_mangle]
pub extern "C" fn lanshare_settings_get_username() -> *mut c_char {
    let username = settings::load().map(|s| s.username);
    to_c(serde_json::json!({ "username": username }).to_string())
}

/// Salva o nome de usuário em %APPDATA%\LANShare\settings.json.
/// Retorna {"ok":true,"username":".."} (nome já limpo) ou {"ok":false,"error":".."}.
#[no_mangle]
pub extern "C" fn lanshare_settings_set_username(name: *const c_char) -> *mut c_char {
    let Some(text) = read_c_string(name) else {
        return reply_err("Nome inválido.");
    };

    // Sem caracteres de controle, sem espaços nas pontas, no máximo MAX_USERNAME_LEN
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    let username: String = cleaned
        .trim()
        .chars()
        .take(MAX_USERNAME_LEN)
        .collect::<String>()
        .trim()
        .to_string();

    if username.is_empty() {
        return reply_err("O nome não pode ficar vazio.");
    }

    match settings::save(&Settings {
        username: username.clone(),
    }) {
        Ok(()) => to_c(serde_json::json!({ "ok": true, "username": username }).to_string()),
        Err(error) => reply_err(&format!("Não foi possível salvar o nome: {error}")),
    }
}

// ───────────────────────────── Host ─────────────────────────────

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
    let users = host
        .as_ref()
        .map(|engine| engine.users())
        .unwrap_or_default();
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

/// Monitores do computador, como JSON (principal primeiro):
/// [{"index":0,"name":"Monitor 1 - DELL U2419H","model":"DELL U2419H","width":1920,"height":1080,"primary":true}].
/// Não precisa da sala aberta.
#[no_mangle]
pub extern "C" fn lanshare_host_monitors() -> *mut c_char {
    let list = monitors::list_monitors().unwrap_or_default();
    to_c(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()))
}

/// Tira uma foto reduzida de um monitor e copia para `buffer` (BGRA, 4 bytes por pixel).
/// `capacity` = tamanho do buffer em bytes (use 400*240*4). Preenche o tamanho da foto
/// e o tamanho real do monitor. Retorna 1 se deu certo, 0 se não.
/// Leva uma fração de segundo: chame fora da thread da interface.
///
/// # Safety
/// `buffer` deve apontar para `capacity` bytes graváveis; os outros ponteiros, para um u32 cada.
#[no_mangle]
pub unsafe extern "C" fn lanshare_host_monitor_thumbnail(
    index: u32,
    buffer: *mut u8,
    capacity: usize,
    width: *mut u32,
    height: *mut u32,
    source_width: *mut u32,
    source_height: *mut u32,
) -> i32 {
    if buffer.is_null()
        || width.is_null()
        || height.is_null()
        || source_width.is_null()
        || source_height.is_null()
    {
        return 0;
    }

    let Ok(thumb) = monitors::monitor_thumbnail(index as usize) else {
        return 0;
    };
    if thumb.bgra.len() > capacity {
        return 0;
    }

    std::ptr::copy_nonoverlapping(thumb.bgra.as_ptr(), buffer, thumb.bgra.len());
    *width = thumb.width;
    *height = thumb.height;
    *source_width = thumb.source_width;
    *source_height = thumb.source_height;
    1
}

/// Começa a transmitir o monitor principal. `height`: 720 ou 1080; `fps`: 15 ou 30.
#[no_mangle]
pub extern "C" fn lanshare_host_start_stream(height: u32, fps: u32) -> *mut c_char {
    lanshare_host_start_stream_on(height, fps, -1)
}

/// Começa a transmitir um monitor. `monitor`: o "index" da lista de monitores
/// (negativo = principal). `height`: 720 ou 1080; `fps`: 15 ou 30.
#[no_mangle]
pub extern "C" fn lanshare_host_start_stream_on(height: u32, fps: u32, monitor: i32) -> *mut c_char {
    let monitor = usize::try_from(monitor).ok();
    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err("a sala não está aberta"),
        Some(engine) => match engine.start_stream_on(StreamSettings { height, fps }, monitor) {
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

/// Copia a miniatura (preview) da transmissão para `buffer` (BGRA, 4 bytes por pixel).
/// `capacity` = tamanho do buffer em bytes (use 2560*1440*4). Preenche `width` e `height`.
/// Retorna 1 se copiou, 0 se não há miniatura (ou o buffer é pequeno).
///
/// # Safety
/// `buffer` deve apontar para `capacity` bytes graváveis; `width` e `height`, para um u32 cada.
#[no_mangle]
pub unsafe extern "C" fn lanshare_host_preview(
    buffer: *mut u8,
    capacity: usize,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    if buffer.is_null() || width.is_null() || height.is_null() {
        return 0;
    }
    let host = HOST.lock().unwrap();
    let Some(engine) = host.as_ref() else {
        return 0;
    };

    let out = std::slice::from_raw_parts_mut(buffer, capacity);
    match engine.preview_into(out) {
        Some((w, h)) => {
            *width = w;
            *height = h;
            1
        }
        None => 0,
    }
}

/// Fecha a sala (para a transmissão e desconecta todos).
#[no_mangle]
pub extern "C" fn lanshare_host_stop() {
    let engine = HOST.lock().unwrap().take();
    if let Some(engine) = engine {
        let _ = engine.stop();
    }
}

// ───────────────────────────── Client ─────────────────────────────

/// Liga o modo Client (cria o motor). Retorna {"ok":true} ou {"ok":false,"error":".."}.
#[no_mangle]
pub extern "C" fn lanshare_client_start() -> *mut c_char {
    let mut client = lock_client();
    if client.is_some() {
        return reply_err("o modo cliente já está ativo");
    }

    match ClientEngine::start() {
        Ok(engine) => {
            *client = Some(engine);
            reply_ok()
        }
        Err(error) => reply_err(&format!("não foi possível iniciar o cliente: {error}")),
    }
}

/// Procura salas na rede (em segundo plano). O resultado chega no evento "rooms".
#[no_mangle]
pub extern "C" fn lanshare_client_scan() -> *mut c_char {
    with_client(|engine| engine.scan_rooms())
}

/// Conecta numa sala. `address`: "192.168.0.10" ou "192.168.0.10:47800".
/// O andamento chega por eventos (connecting, password_required, connected, rejected...).
#[no_mangle]
pub extern "C" fn lanshare_client_connect(address: *const c_char) -> *mut c_char {
    let Some(address) = read_c_string(address) else {
        return reply_err("endereço inválido");
    };
    with_client(|engine| engine.connect(&address))
}

/// Responde ao evento "password_required" com a senha digitada.
#[no_mangle]
pub extern "C" fn lanshare_client_submit_password(password: *const c_char) -> *mut c_char {
    let Some(password) = read_c_string(password) else {
        return reply_err("senha inválida");
    };
    with_client(|engine| engine.submit_password(password))
}

/// Desconecta da sala (ou cancela a tentativa). O fim chega no evento "disconnected".
/// Retorna 1 se havia conexão, 0 se não.
#[no_mangle]
pub extern "C" fn lanshare_client_disconnect() -> i32 {
    let client = lock_client();
    client.as_ref().map(|e| e.disconnect()).unwrap_or(false) as i32
}

/// Próximo evento do Client como JSON (ex.: {"type":"connected",...}), ou NULL se não há nenhum.
#[no_mangle]
pub extern "C" fn lanshare_client_poll_event() -> *mut c_char {
    let client = lock_client();
    match client.as_ref().and_then(|engine| engine.try_next_event()) {
        Some(event) => to_c(event.to_string()),
        None => std::ptr::null_mut(),
    }
}

/// Copia a imagem recebida do Host para `buffer` (BGRA, 4 bytes por pixel).
/// Retorna 1 só se há uma imagem NOVA desde a última chamada; 0 se não.
///
/// # Safety
/// `buffer` deve apontar para `capacity` bytes graváveis; `width` e `height`, para um u32 cada.
#[no_mangle]
pub unsafe extern "C" fn lanshare_client_frame(
    buffer: *mut u8,
    capacity: usize,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    if buffer.is_null() || width.is_null() || height.is_null() {
        return 0;
    }
    let client = lock_client();
    let Some(engine) = client.as_ref() else {
        return 0;
    };

    let out = std::slice::from_raw_parts_mut(buffer, capacity);
    match engine.frame_into(out) {
        Some((w, h)) => {
            *width = w;
            *height = h;
            1
        }
        None => 0,
    }
}

/// Desliga o modo Client (desconecta e encerra o motor).
#[no_mangle]
pub extern "C" fn lanshare_client_stop() {
    let engine = lock_client().take();
    if let Some(engine) = engine {
        engine.stop();
    }
}

fn lock_client() -> MutexGuard<'static, Option<ClientEngine>> {
    CLIENT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Roda uma ação no motor do Client e devolve {"ok":true} ou {"ok":false,"error":".."}.
fn with_client(action: impl FnOnce(&ClientEngine) -> Result<(), String>) -> *mut c_char {
    let client = lock_client();
    match client.as_ref() {
        None => reply_err("o modo cliente não está ativo"),
        Some(engine) => match action(engine) {
            Ok(()) => reply_ok(),
            Err(error) => reply_err(&error),
        },
    }
}

// ───────────────────────────── Ajudantes ─────────────────────────────

fn read_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
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
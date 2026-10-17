//! lanshare-ffi: ponte entre o motor Rust do LANShare e a interface em C# (Avalonia).
//!
//! Só funções em C ABI (`extern "C"`). A interface chama cada uma via P/Invoke.
//! Textos que voltam do Rust (JSON) são alocados aqui e DEVEM ser liberados
//! pelo C# com `lanshare_free_string`.

use lanshare_client::{audio_devices, ClientEngine};
use lanshare_core::i18n::{self, tr, Language};
use lanshare_core::settings::{self, Settings};
use lanshare_core::sfx;
use lanshare_host::monitors::{self, Thumbnail};
use lanshare_host::{app_windows, CaptureTarget, HostConfig, HostEngine, StreamSettings};
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

/// Define o idioma das mensagens do motor: "pt", "en" ou "es".
/// Retorna 1 se o idioma foi aceito, 0 se não (fica o anterior).
#[no_mangle]
pub extern "C" fn lanshare_set_language(code: *const c_char) -> i32 {
    match read_c_string(code).and_then(|text| Language::from_code(&text)) {
        Some(language) => {
            i18n::set_language(language);
            1
        }
        None => 0,
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
        return reply_err(tr("Nome inválido.", "Invalid name.", "Nombre no válido."));
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
        return reply_err(tr(
            "O nome não pode ficar vazio.",
            "The name cannot be empty.",
            "El nombre no puede quedar vacío.",
        ));
    }

    match settings::save(&Settings {
        username: username.clone(),
    }) {
        Ok(()) => to_c(serde_json::json!({ "ok": true, "username": username }).to_string()),
        Err(error) => reply_err(&format!(
            "{}: {error}",
            tr(
                "Não foi possível salvar o nome",
                "Could not save the name",
                "No se pudo guardar el nombre"
            )
        )),
    }
}

// ───────────────────────────── Host ─────────────────────────────

/// Abre a sala. `config_json`: {"password":"..","max_clients":4,"room_name":".."} (tudo opcional).
/// Retorna JSON: {"ok":true} ou {"ok":false,"error":"..."}.
#[no_mangle]
pub extern "C" fn lanshare_host_start(config_json: *const c_char) -> *mut c_char {
    let mut host = HOST.lock().unwrap();
    if host.is_some() {
        return reply_err(tr(
            "A sala já está aberta.",
            "The room is already open.",
            "La sala ya está abierta.",
        ));
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
        Err(error) => {
            return reply_err(&format!(
                "{}: {error}",
                tr("Configuração inválida", "Invalid settings", "Configuración no válida")
            ))
        }
    };

    match HostEngine::start(config) {
        Ok(engine) => {
            *host = Some(engine);
            reply_ok()
        }
        Err(error) => reply_err(&format!(
            "{}: {error}",
            tr(
                "Não foi possível abrir a sala",
                "Could not open the room",
                "No se pudo abrir la sala"
            )
        )),
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
        None => reply_err(room_not_open()),
        Some(engine) => match engine.kick(id) {
            Ok(name) => to_c(serde_json::json!({ "ok": true, "name": name }).to_string()),
            Err(error) => reply_err(&error),
        },
    }
}

/// Define o avatar do Host (PNG em base64; vazio ou NULL = sem avatar) e avisa os Clients.
#[no_mangle]
pub extern "C" fn lanshare_host_set_avatar(png_base64: *const c_char) -> *mut c_char {
    let avatar = read_c_string(png_base64).filter(|text| !text.is_empty());
    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err(room_not_open()),
        Some(engine) => match engine.set_avatar(avatar) {
            Ok(()) => reply_ok(),
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
    let result = monitors::monitor_thumbnail(index as usize);
    write_thumbnail(result.ok(), buffer, capacity, width, height, source_width, source_height)
}

/// Janelas que dá para transmitir, como JSON (a mais recente primeiro):
/// [{"id":123456,"title":"Documento - Bloco de Notas","minimized":false}].
#[no_mangle]
pub extern "C" fn lanshare_host_windows() -> *mut c_char {
    let list = app_windows::list_windows();
    to_c(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()))
}

/// Igual a `lanshare_host_monitor_thumbnail`, mas para uma janela (pelo "id" da lista).
///
/// # Safety
/// `buffer` deve apontar para `capacity` bytes graváveis; os outros ponteiros, para um u32 cada.
#[no_mangle]
pub unsafe extern "C" fn lanshare_host_window_thumbnail(
    id: u64,
    buffer: *mut u8,
    capacity: usize,
    width: *mut u32,
    height: *mut u32,
    source_width: *mut u32,
    source_height: *mut u32,
) -> i32 {
    let result = app_windows::window_thumbnail(id);
    write_thumbnail(result.ok(), buffer, capacity, width, height, source_width, source_height)
}

/// Copia o ícone da janela para `buffer`: 32x32 pixels BGRA com alfa pré-multiplicado
/// (use 32*32*4 bytes). Retorna 1 se a janela tem ícone, 0 se não.
///
/// # Safety
/// `buffer` deve apontar para `capacity` bytes graváveis.
#[no_mangle]
pub unsafe extern "C" fn lanshare_host_window_icon(id: u64, buffer: *mut u8, capacity: usize) -> i32 {
    if buffer.is_null() {
        return 0;
    }
    let Some(pixels) = app_windows::window_icon(id) else {
        return 0;
    };
    if pixels.len() > capacity {
        return 0;
    }
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), buffer, pixels.len());
    1
}

/// Começa a transmitir o monitor principal. `height`: 720 ou 1080; `fps`: 15 ou 30.
#[no_mangle]
pub extern "C" fn lanshare_host_start_stream(height: u32, fps: u32) -> *mut c_char {
    start_stream(height, fps, CaptureTarget::PrimaryMonitor)
}

/// Começa a transmitir um monitor. `monitor`: o "index" da lista de monitores
/// (negativo = principal). `height`: 720 ou 1080; `fps`: 15 ou 30.
#[no_mangle]
pub extern "C" fn lanshare_host_start_stream_on(height: u32, fps: u32, monitor: i32) -> *mut c_char {
    start_stream(height, fps, monitor_target(monitor))
}

/// Começa a transmitir uma janela (o "id" da lista de janelas).
#[no_mangle]
pub extern "C" fn lanshare_host_start_stream_window(height: u32, fps: u32, id: u64) -> *mut c_char {
    start_stream(height, fps, CaptureTarget::Window(id))
}

/// Troca a transmissão em andamento para outro monitor (negativo = principal), sem parar.
#[no_mangle]
pub extern "C" fn lanshare_host_switch_monitor(monitor: i32) -> *mut c_char {
    switch_source(monitor_target(monitor))
}

/// Troca a transmissão em andamento para uma janela (o "id" da lista), sem parar.
#[no_mangle]
pub extern "C" fn lanshare_host_switch_window(id: u64) -> *mut c_char {
    switch_source(CaptureTarget::Window(id))
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

/// Toca um efeito sonoro do Host para todos os Clients (a interface já tocou ele no Host).
/// `pcm`: `len` amostras float intercaladas, 48 kHz, estéreo (no máximo MAX_SAMPLES do sfx.rs).
/// Retorna {"ok":true} ou {"ok":false,"error":".."}. Leva alguns milissegundos: chame fora da thread da interface.
///
/// # Safety
/// `pcm` deve apontar para `len` floats legíveis.
#[no_mangle]
pub unsafe extern "C" fn lanshare_host_play_sfx(name: *const c_char, pcm: *const f32, len: usize) -> *mut c_char {
    let packets = match encode_pcm(pcm, len) {
        Ok(packets) => packets,
        Err(error) => return reply_err(&error),
    };
    let name = read_c_string(name).unwrap_or_default();

    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err(room_not_open()),
        Some(engine) => match engine.play_sfx(&name, packets) {
            Ok(()) => reply_ok(),
            Err(error) => reply_err(&error),
        },
    }
}

/// Copia para `buffer` o efeito sonoro avisado no evento "sfx_played" (pelo "sfx_id").
/// Retorna quantas amostras foram copiadas (float intercalado, 48 kHz, estéreo);
/// -1 se o som não existe (já buscado ou descartado); -2 se `capacity` é pequena demais.
/// Cada som só pode ser buscado uma vez.
///
/// # Safety
/// `buffer` deve apontar para `capacity` floats graváveis.
#[no_mangle]
pub unsafe extern "C" fn lanshare_host_take_sfx(sfx_id: u64, buffer: *mut f32, capacity: usize) -> i64 {
    let samples = {
        let host = HOST.lock().unwrap();
        host.as_ref().and_then(|engine| engine.take_sfx(sfx_id))
    };
    write_pcm(samples, buffer, capacity)
}

/// Fecha a sala (para a transmissão e desconecta todos).
#[no_mangle]
pub extern "C" fn lanshare_host_stop() {
    let engine = HOST.lock().unwrap().take();
    if let Some(engine) = engine {
        let _ = engine.stop();
    }
}

/// Índice da lista de monitores -> o que capturar (negativo = principal).
fn monitor_target(monitor: i32) -> CaptureTarget {
    match usize::try_from(monitor) {
        Ok(index) => CaptureTarget::Monitor(index),
        Err(_) => CaptureTarget::PrimaryMonitor,
    }
}

fn start_stream(height: u32, fps: u32, target: CaptureTarget) -> *mut c_char {
    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err(room_not_open()),
        Some(engine) => match engine.start_stream_on(StreamSettings { height, fps }, target) {
            Ok(()) => reply_ok(),
            Err(error) => reply_err(&error),
        },
    }
}

fn switch_source(target: CaptureTarget) -> *mut c_char {
    let host = HOST.lock().unwrap();
    match host.as_ref() {
        None => reply_err(room_not_open()),
        Some(engine) => match engine.switch_source(target) {
            Ok(()) => reply_ok(),
            Err(error) => reply_err(&error),
        },
    }
}

/// Copia uma miniatura (de monitor ou janela) para o buffer da interface.
unsafe fn write_thumbnail(
    thumb: Option<Thumbnail>,
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
    let Some(thumb) = thumb else {
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

// ───────────────────────────── Client ─────────────────────────────

/// Liga o modo Client (cria o motor). Retorna {"ok":true} ou {"ok":false,"error":".."}.
#[no_mangle]
pub extern "C" fn lanshare_client_start() -> *mut c_char {
    let mut client = lock_client();
    if client.is_some() {
        return reply_err(tr(
            "O modo cliente já está ativo.",
            "Client mode is already active.",
            "El modo cliente ya está activo.",
        ));
    }

    match ClientEngine::start() {
        Ok(engine) => {
            *client = Some(engine);
            reply_ok()
        }
        Err(error) => reply_err(&format!(
            "{}: {error}",
            tr(
                "Não foi possível iniciar o modo cliente",
                "Could not start client mode",
                "No se pudo iniciar el modo cliente"
            )
        )),
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
        return reply_err(tr("Endereço inválido.", "Invalid address.", "Dirección no válida."));
    };
    with_client(|engine| engine.connect(&address))
}

/// Responde ao evento "password_required" com a senha digitada.
#[no_mangle]
pub extern "C" fn lanshare_client_submit_password(password: *const c_char) -> *mut c_char {
    let Some(password) = read_c_string(password) else {
        return reply_err(tr("Senha inválida.", "Invalid password.", "Contraseña no válida."));
    };
    with_client(|engine| engine.submit_password(password))
}

/// Define o meu avatar (PNG em base64; vazio ou NULL = sem avatar).
/// Vai ao Host ao entrar numa sala (ou na hora, se já estiver numa).
#[no_mangle]
pub extern "C" fn lanshare_client_set_avatar(png_base64: *const c_char) -> *mut c_char {
    let avatar = read_c_string(png_base64).filter(|text| !text.is_empty());
    with_client(|engine| engine.set_avatar(avatar))
}

/// Escolhe por onde sai o som das transmissões: o "id" da lista de `lanshare_audio_output_devices`.
/// Vazio ou NULL = padrão do Windows (acompanha se o padrão mudar). Se já houver som tocando,
/// ele muda de saída na hora.
#[no_mangle]
pub extern "C" fn lanshare_client_set_audio_output(device_id: *const c_char) -> *mut c_char {
    let device_id = read_c_string(device_id).filter(|text| !text.trim().is_empty());
    with_client(|engine| {
        engine.set_audio_output(device_id);
        Ok(())
    })
}

/// Volume do som das transmissões: `percent` de 0 a 100; `muted` diferente de 0 = mudo.
/// Muda na hora, mesmo com som tocando.
#[no_mangle]
pub extern "C" fn lanshare_client_set_volume(percent: u32, muted: i32) -> *mut c_char {
    with_client(|engine| {
        engine.set_volume(percent.min(100), muted != 0);
        Ok(())
    })
}

/// Toca um efeito sonoro para a sala: vai ao Host, que repassa a todos (a interface já tocou ele aqui).
/// `pcm`: `len` amostras float intercaladas, 48 kHz, estéreo (no máximo MAX_SAMPLES do sfx.rs).
/// Retorna {"ok":true} ou {"ok":false,"error":".."}. Leva alguns milissegundos: chame fora da thread da interface.
///
/// # Safety
/// `pcm` deve apontar para `len` floats legíveis.
#[no_mangle]
pub unsafe extern "C" fn lanshare_client_play_sfx(name: *const c_char, pcm: *const f32, len: usize) -> *mut c_char {
    let packets = match encode_pcm(pcm, len) {
        Ok(packets) => packets,
        Err(error) => return reply_err(&error),
    };
    let name = read_c_string(name).unwrap_or_default();
    with_client(|engine| engine.play_sfx(&name, packets))
}

/// Igual a `lanshare_host_take_sfx`, para o Client.
///
/// # Safety
/// `buffer` deve apontar para `capacity` floats graváveis.
#[no_mangle]
pub unsafe extern "C" fn lanshare_client_take_sfx(sfx_id: u64, buffer: *mut f32, capacity: usize) -> i64 {
    let samples = {
        let client = lock_client();
        client.as_ref().and_then(|engine| engine.take_sfx(sfx_id))
    };
    write_pcm(samples, buffer, capacity)
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
        None => reply_err(tr(
            "O modo cliente não está ativo.",
            "Client mode is not active.",
            "El modo cliente no está activo.",
        )),
        Some(engine) => match action(engine) {
            Ok(()) => reply_ok(),
            Err(error) => reply_err(&error),
        },
    }
}

// ───────────────────────────── Áudio ─────────────────────────────

/// Dispositivos de saída de áudio (por onde o Client toca o som das transmissões), como JSON:
/// {"ok":true,"devices":[{"id":"{0.0.0.00000000}.{...}","name":"Alto-falantes (Realtek)","default":true}]}
/// ou {"ok":false,"error":".."}. O padrão do Windows vem primeiro. Não precisa do modo Client ativo.
/// Leva alguns milissegundos: chame fora da thread da interface.
#[no_mangle]
pub extern "C" fn lanshare_audio_output_devices() -> *mut c_char {
    match audio_devices::list_output_devices() {
        Ok(devices) => {
            let list: Vec<_> = devices
                .iter()
                .map(|d| serde_json::json!({ "id": d.id, "name": d.name, "default": d.is_default }))
                .collect();
            to_c(serde_json::json!({ "ok": true, "devices": list }).to_string())
        }
        Err(error) => reply_err(&error),
    }
}

// ───────────────────────────── Ajudantes ─────────────────────────────

fn room_not_open() -> &'static str {
    tr(
        "A sala não está aberta.",
        "The room is not open.",
        "La sala no está abierta.",
    )
}

/// Lê as amostras que vieram da interface e comprime em Opus (antes de pegar o lock do motor:
/// comprimir leva alguns milissegundos e a interface não pode ficar esperando o lock).
///
/// # Safety
/// `pcm` deve apontar para `len` floats legíveis.
unsafe fn encode_pcm(pcm: *const f32, len: usize) -> Result<Vec<Vec<u8>>, String> {
    if pcm.is_null() || len == 0 || len > sfx::MAX_SAMPLES {
        return Err(tr("Som inválido.", "Invalid sound.", "Sonido no válido.").to_string());
    }
    sfx::encode(std::slice::from_raw_parts(pcm, len))
}

/// Copia o som buscado para o buffer da interface (veja `lanshare_host_take_sfx`).
///
/// # Safety
/// `buffer` deve apontar para `capacity` floats graváveis.
unsafe fn write_pcm(samples: Option<Vec<f32>>, buffer: *mut f32, capacity: usize) -> i64 {
    let Some(samples) = samples else {
        return -1;
    };
    if buffer.is_null() || samples.len() > capacity {
        return -2;
    }
    std::ptr::copy_nonoverlapping(samples.as_ptr(), buffer, samples.len());
    samples.len() as i64
}

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



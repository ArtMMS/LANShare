//! Apps com som no PC (sessões de áudio do Windows) e a lista de apps que NÃO transmitem som.
//!
//! - `list_audio_apps`: quem tem uma sessão de áudio aberta agora (a lista "Adicionar app" das
//!   Configurações). O mesmo app com vários processos (ex.: Discord) aparece uma vez só.
//! - `set_excluded_apps`: os .exe bloqueados (Discord, Teams...). Vale para a transmissão de
//!   monitor inteiro: o som desses apps fica de fora.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{CloseHandle, S_OK};
use windows::Win32::Media::Audio::{
    eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Os .exe que não transmitem som (sempre em minúsculas).
static EXCLUDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Um app com som no PC, como a interface o enxerga.
#[derive(Debug, Clone, Serialize)]
pub struct AudioApp {
    /// Nome do executável, em minúsculas (ex.: "discord.exe"). É o que fica salvo na lista.
    pub exe: String,
    /// Nome para mostrar (ex.: "Discord").
    pub name: String,
    /// true = está tocando som agora.
    pub active: bool,
    /// Processos do app que têm sessão de áudio.
    #[serde(skip)]
    pub pids: Vec<u32>,
}

/// Troca a lista de .exe que não transmitem som.
pub fn set_excluded_apps(exes: Vec<String>) {
    let mut list: Vec<String> = exes
        .into_iter()
        .map(|exe| exe.trim().to_lowercase())
        .filter(|exe| !exe.is_empty())
        .collect();
    list.sort();
    list.dedup();
    *EXCLUDED.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

/// A lista de .exe que não transmitem som.
pub fn excluded_apps() -> Vec<String> {
    EXCLUDED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Esse .exe está na lista dos que não transmitem som?
pub fn is_excluded(exe: &str) -> bool {
    let exe = exe.to_lowercase();
    EXCLUDED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|item| *item == exe)
}

/// Apps com sessão de áudio aberta (os que tocam som agora primeiro). Não inclui o LANShare
/// nem os sons do sistema. Leva alguns milissegundos: não use na thread da interface.
pub fn list_audio_apps() -> Result<Vec<AudioApp>, String> {
    std::thread::spawn(|| {
        let _com = ComGuard::new()?;
        unsafe { enumerate() }
    })
    .join()
    .map_err(|_| "a listagem de apps com som terminou com panico".to_string())?
}

/// Liga o COM nesta thread enquanto existir.
struct ComGuard;

impl ComGuard {
    fn new() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|e| format!("CoInitializeEx: {e}"))?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Passa por todas as saídas de áudio e junta as sessões de cada app.
unsafe fn enumerate() -> Result<Vec<AudioApp>, String> {
    let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        .map_err(|e| format!("IMMDeviceEnumerator: {e}"))?;
    let collection = devices
        .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;
    let count = collection.GetCount().map_err(|e| format!("GetCount: {e}"))?;

    let own_pid = std::process::id();
    let mut apps: BTreeMap<String, AudioApp> = BTreeMap::new();

    for index in 0..count {
        let Ok(device) = collection.Item(index) else { continue };
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            continue;
        };
        let Ok(sessions) = manager.GetSessionEnumerator() else { continue };

        for item in 0..sessions.GetCount().unwrap_or(0) {
            let Ok(control) = sessions.GetSession(item) else { continue };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };

            // "Sons do sistema" (avisos do Windows) não é um app
            if control2.IsSystemSoundsSession() == S_OK {
                continue;
            }

            let Ok(pid) = control2.GetProcessId() else { continue };
            if pid == 0 || pid == own_pid {
                continue;
            }
            let Some(exe) = process_exe(pid) else { continue };

            let active = control.GetState().is_ok_and(|state| state == AudioSessionStateActive);
            let key = exe.to_lowercase();
            let app = apps.entry(key.clone()).or_insert_with(|| AudioApp {
                exe: key,
                name: display_name(&exe),
                active: false,
                pids: Vec::new(),
            });
            app.active |= active;
            if !app.pids.contains(&pid) {
                app.pids.push(pid);
            }
        }
    }

    let mut list: Vec<AudioApp> = apps.into_values().collect();
    list.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(list)
}

/// Nome do .exe de um processo (ex.: "Discord.exe"). None = processo protegido ou já fechado.
pub fn process_exe(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }
}

/// "discord.exe" -> "Discord".
fn display_name(exe: &str) -> String {
    let stem = Path::new(exe)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| exe.to_string());
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => stem,
    }
}
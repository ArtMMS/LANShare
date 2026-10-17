//! Dispositivos de saída de áudio (alto-falantes, fones...) para a escolha nas Configurações.
//! É por um deles que o Client toca o som da transmissão que está assistindo (audio_player.rs).

use lanshare_core::i18n::tr;
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

/// Um dispositivo de saída, como a interface o enxerga.
#[derive(Debug, Clone)]
pub struct OutputDevice {
    /// Identificador do Windows (é o que fica salvo nas preferências).
    pub id: String,
    /// Ex.: "Alto-falantes (Realtek(R) Audio)".
    pub name: String,
    /// É o dispositivo padrão do Windows agora?
    pub is_default: bool,
}

/// Lista os dispositivos de saída ativos, com o padrão do Windows primeiro.
/// Roda numa thread própria: a thread da interface (C#) usa outro modo de COM.
pub fn list_output_devices() -> Result<Vec<OutputDevice>, String> {
    std::thread::spawn(|| {
        let _com = ComGuard::new()?;
        enumerate()
    })
    .join()
    .map_err(|_| "a listagem de dispositivos de audio terminou com panico".to_string())?
}

/// Liga o COM nesta thread enquanto existir.
pub(crate) struct ComGuard;

impl ComGuard {
    pub(crate) fn new() -> Result<Self, String> {
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

fn enumerate() -> Result<Vec<OutputDevice>, String> {
    let fail = |e: windows::core::Error| {
        format!(
            "{}: {e}",
            tr(
                "Não foi possível listar os dispositivos de áudio",
                "Could not list the audio devices",
                "No se pudieron listar los dispositivos de audio"
            )
        )
    };

    unsafe {
        let devices_enum = enumerator().map_err(fail)?;

        // Sem nenhum dispositivo de saída, o Windows não tem padrão: não é erro
        let default_id = devices_enum
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|device| device_id(&device));

        let collection = devices_enum
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(fail)?;
        let count = collection.GetCount().map_err(fail)?;

        let mut devices = Vec::with_capacity(count as usize);
        for index in 0..count {
            let Ok(device) = collection.Item(index) else {
                continue;
            };
            let Some(id) = device_id(&device) else {
                continue;
            };
            let name = friendly_name(&device).unwrap_or_else(|| {
                tr("Dispositivo de áudio", "Audio device", "Dispositivo de audio").to_string()
            });
            let is_default = default_id.as_deref() == Some(id.as_str());
            devices.push(OutputDevice {
                id,
                name,
                is_default,
            });
        }

        // Padrão primeiro; os outros em ordem alfabética
        devices.sort_by(|a, b| {
            b.is_default
                .cmp(&a.is_default)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(devices)
    }
}

/// Cria o enumerador de dispositivos do Windows (precisa do COM ligado na thread).
pub(crate) fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

/// A saída que deve tocar o som agora: a escolhida, se estiver conectada;
/// senão (ou sem escolha), a padrão do Windows. None = o PC não tem nenhuma saída.
pub(crate) fn resolve_output(
    enumerator: &IMMDeviceEnumerator,
    chosen: Option<&str>,
) -> Option<(String, IMMDevice)> {
    if let Some(id) = chosen {
        let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        let device = unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) }.ok();
        if let Some(device) = device {
            let active = unsafe { device.GetState() }.is_ok_and(|state| state == DEVICE_STATE_ACTIVE);
            if active {
                return Some((id.to_string(), device));
            }
        }
    }

    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }.ok()?;
    let id = device_id(&device)?;
    Some((id, device))
}

/// Identificador do dispositivo (o texto que o Windows usa para achá-lo de novo).
pub(crate) fn device_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let raw = device.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const c_void));
        id.filter(|text| !text.is_empty())
    }
}

/// Nome do dispositivo como aparece no Windows (ex.: "Fones de ouvido (HyperX)").
fn friendly_name(device: &IMMDevice) -> Option<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = value.to_string().trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

//! Janelas do Host: lista as janelas abertas (as mesmas da barra de tarefas),
//! com título, ícone e foto (usado na aba "Janela específica").
//! Também descobre qual processo toca o som de uma janela (para transmitir só o áudio dela).

use crate::capture::{CaptureTarget, ScreenCapture};
use crate::monitors::{shrink, Thumbnail};
use serde::Serialize;
use std::error::Error;
use std::ffi::c_void;
use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, FindWindowExW, GetClassLongPtrW, GetClassNameW, GetTopWindow, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SendMessageTimeoutW, DI_NORMAL, GCLP_HICON, GCLP_HICONSM, GWL_EXSTYLE,
    GW_HWNDNEXT, GW_OWNER, HICON, SMTO_ABORTIFHUNG, WM_GETICON, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

type BoxError = Box<dyn Error + Send + Sync>;

/// Lado do ícone devolvido à interface (BGRA). A interface (C#) reserva este tamanho: mantenha igual.
pub const ICON_SIZE: usize = 32;

/// Limites para a lista não crescer sem fim.
const MAX_WINDOWS: usize = 60;
const MAX_SCAN: usize = 4096;

/// Tempo máximo esperando a janela entregar a primeira imagem.
const THUMB_TIMEOUT: Duration = Duration::from_millis(1500);

/// Tempo máximo esperando uma janela responder qual é o ícone dela.
const ICON_TIMEOUT_MS: u32 = 100;

// Tipos de ícone do WM_GETICON
const ICON_SMALL: usize = 0;
const ICON_BIG: usize = 1;
const ICON_SMALL2: usize = 2;

/// Janelas do próprio Windows (área de trabalho, barra de tarefas) que nunca entram na lista.
const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// Moldura dos apps da Microsoft Store (pertence ao ApplicationFrameHost, não ao app).
const STORE_FRAME_CLASS: &str = "ApplicationFrameWindow";

/// Uma janela, como a interface a enxerga.
#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    /// Identificador da janela (HWND). É o que a transmissão usa para achá-la.
    pub id: u64,
    pub title: String,
    pub minimized: bool,
}

/// Lista as janelas que dá para transmitir, da mais recente para a mais antiga (como o Alt+Tab).
pub fn list_windows() -> Vec<WindowInfo> {
    let own_pid = std::process::id();
    let mut found = Vec::new();

    let mut current = unsafe { GetTopWindow(None) }.ok();
    let mut scanned = 0;
    while let Some(hwnd) = current {
        scanned += 1;
        if scanned > MAX_SCAN || found.len() >= MAX_WINDOWS {
            break;
        }
        if let Some(info) = describe(hwnd, own_pid) {
            found.push(info);
        }
        current = unsafe { GetWindow(hwnd, GW_HWNDNEXT) }.ok();
    }

    found
}

/// Processo que toca o som da janela (o som dele e dos processos filhos é o que vai na transmissão).
/// Apps da Microsoft Store: a janela de fora é do ApplicationFrameHost; o app de verdade
/// é a CoreWindow que fica dentro dela.
pub fn window_process_id(id: u64) -> Option<u32> {
    let hwnd = to_hwnd(id);

    if class_name(hwnd) == STORE_FRAME_CLASS {
        let core = unsafe {
            FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), PCWSTR::null())
        };
        if let Some(pid) = core.ok().and_then(process_of) {
            return Some(pid);
        }
    }

    process_of(hwnd)
}

fn process_of(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
    (pid != 0).then_some(pid)
}

/// Devolve a janela se ela é uma "janela de verdade" (aparece na barra de tarefas).
fn describe(hwnd: HWND, own_pid: u32) -> Option<WindowInfo> {
    unsafe {
        if hwnd.is_invalid() || !IsWindowVisible(hwnd).as_bool() {
            return None;
        }

        // Janelas de ferramenta e janelas "filhas" de outra (diálogos) ficam de fora
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
        let owned = GetWindow(hwnd, GW_OWNER).is_ok();
        if owned && ex_style & WS_EX_APPWINDOW.0 == 0 {
            return None;
        }

        // Apps da Microsoft Store suspensos ficam "invisíveis" sem estar escondidos
        if is_cloaked(hwnd) {
            return None;
        }

        // O próprio LANShare não aparece
        if process_of(hwnd) == Some(own_pid) {
            return None;
        }

        if SHELL_CLASSES.contains(&class_name(hwnd).as_str()) {
            return None;
        }

        let title = window_title(hwnd)?;
        Some(WindowInfo {
            id: hwnd.0 as usize as u64,
            title,
            minimized: IsIconic(hwnd).as_bool(),
        })
    }
}

/// Tira uma foto da janela e a reduz para a miniatura.
/// Leva uma fração de segundo; não use na thread da interface.
pub fn window_thumbnail(id: u64) -> Result<Thumbnail, BoxError> {
    // Janela minimizada não entrega imagem: nem tenta (economiza a espera)
    if unsafe { IsIconic(to_hwnd(id)) }.as_bool() {
        return Err("a janela esta minimizada".into());
    }

    let capture = ScreenCapture::start_quiet(CaptureTarget::Window(id))?;
    let received = capture.frames.recv_timeout(THUMB_TIMEOUT);
    let _ = capture.stop();
    let frame = received.map_err(|_| "a janela nao entregou nenhuma imagem")?;
    shrink(&frame)
}

/// Ícone do app dono da janela: ICON_SIZE x ICON_SIZE pixels BGRA (alfa pré-multiplicado).
pub fn window_icon(id: u64) -> Option<Vec<u8>> {
    unsafe {
        let icon = find_icon(to_hwnd(id))?;
        render_icon(icon)
    }
}

fn to_hwnd(id: u64) -> HWND {
    HWND(id as usize as *mut c_void)
}

fn window_title(hwnd: HWND) -> Option<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
        let title = String::from_utf16_lossy(&buffer[..copied]).trim().to_string();
        (!title.is_empty()).then_some(title)
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..len])
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    result.is_ok() && cloaked != 0
}

/// Pergunta o ícone à janela; se ela não responder, usa o ícone da classe dela.
unsafe fn find_icon(hwnd: HWND) -> Option<HICON> {
    for kind in [ICON_BIG, ICON_SMALL2, ICON_SMALL] {
        let mut result = 0usize;
        let answered = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(kind),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            ICON_TIMEOUT_MS,
            Some(&mut result as *mut usize),
        );
        if answered.0 != 0 && result != 0 {
            return Some(HICON(result as *mut c_void));
        }
    }

    for index in [GCLP_HICON, GCLP_HICONSM] {
        let handle = GetClassLongPtrW(hwnd, index);
        if handle != 0 {
            return Some(HICON(handle as *mut c_void));
        }
    }

    None
}

/// Desenha o ícone numa imagem em memória e devolve os pixels.
unsafe fn render_icon(icon: HICON) -> Option<Vec<u8>> {
    let size = ICON_SIZE as i32;

    let dc = CreateCompatibleDC(None);
    if dc.is_invalid() {
        return None;
    }

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // negativo = linhas de cima para baixo
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(bitmap) if !bits.is_null() => bitmap,
        _ => {
            let _ = DeleteDC(dc);
            return None;
        }
    };

    let previous = SelectObject(dc, bitmap.into());
    let drawn = DrawIconEx(dc, 0, 0, icon, size, size, 0, None, DI_NORMAL).is_ok();
    let mut pixels = std::slice::from_raw_parts(bits as *const u8, ICON_SIZE * ICON_SIZE * 4).to_vec();

    SelectObject(dc, previous);
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(dc);

    if !drawn {
        return None;
    }

    // Ícones antigos não têm transparência: vêm com alfa 0 em tudo
    if pixels.chunks_exact(4).all(|px| px[3] == 0) {
        for px in pixels.chunks_exact_mut(4) {
            if px[0] != 0 || px[1] != 0 || px[2] != 0 {
                px[3] = 255;
            }
        }
    }

    Some(pixels)
}
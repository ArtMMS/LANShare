//! lanshare-ffi: ponte entre o motor Rust do LANShare e a interface em C# (Avalonia).
//!
//! Aqui só entram funções em C ABI (`extern "C"`). A interface chama cada uma via P/Invoke.

use std::os::raw::c_char;

/// Versão do motor Rust, como texto C (UTF-8 terminado em `\0`).
/// O ponteiro aponta para memória estática: quem chama NÃO deve liberá-lo.
#[no_mangle]
pub extern "C" fn lanshare_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn version_is_a_valid_c_string() {
        let text = unsafe { CStr::from_ptr(lanshare_version()) };
        assert_eq!(text.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }
}
//! Idioma das mensagens do motor (erros e avisos que a interface mostra).
//! A interface escolhe o idioma pela FFI; o modo terminal continua em português.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Portuguese,
    English,
    Spanish,
}

impl Language {
    /// "pt", "en" ou "es" (aceita também "pt-BR", "en-US"...). Outro valor = None.
    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim().to_lowercase();
        match code.split(['-', '_']).next().unwrap_or("") {
            "pt" => Some(Self::Portuguese),
            "en" => Some(Self::English),
            "es" => Some(Self::Spanish),
            _ => None,
        }
    }
}

/// 0 = português (padrão), 1 = inglês, 2 = espanhol.
static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    let value = match language {
        Language::Portuguese => 0,
        Language::English => 1,
        Language::Spanish => 2,
    };
    CURRENT.store(value, Ordering::Relaxed);
}

pub fn language() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::English,
        2 => Language::Spanish,
        _ => Language::Portuguese,
    }
}

/// Escolhe o texto no idioma atual.
pub fn tr(pt: &'static str, en: &'static str, es: &'static str) -> &'static str {
    match language() {
        Language::Portuguese => pt,
        Language::English => en,
        Language::Spanish => es,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigos_de_idioma() {
        assert_eq!(Language::from_code("pt-BR"), Some(Language::Portuguese));
        assert_eq!(Language::from_code("EN"), Some(Language::English));
        assert_eq!(Language::from_code("es_ES"), Some(Language::Spanish));
        assert_eq!(Language::from_code("fr"), None);
    }
}

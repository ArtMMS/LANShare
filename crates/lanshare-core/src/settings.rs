use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use std::{env, fs};

#[derive(Serialize, Deserialize)]
pub struct Settings {
    pub username: String,
}

// %APPDATA%\LANShare\settings.json
fn settings_path() -> io::Result<PathBuf> {
    let appdata = env::var_os("APPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA não encontrado"))?;
    Ok(PathBuf::from(appdata).join("LANShare").join("settings.json"))
}

// Arquivo ausente, quebrado ou com nome vazio = trata como primeira vez
pub fn load() -> Option<Settings> {
    let text = fs::read_to_string(settings_path().ok()?).ok()?;
    let settings: Settings = serde_json::from_str(&text).ok()?;
    if settings.username.trim().is_empty() {
        None
    } else {
        Some(settings)
    }
}

pub fn save(settings: &Settings) -> io::Result<()> {
    let path = settings_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?; // cria a pasta LANShare se não existir
    }
    let json = serde_json::to_string_pretty(settings)?;
    fs::write(path, json)
}

fn ask_username() -> String {
    loop {
        print!("Qual é o seu nome? ");
        io::stdout().flush().ok();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) | Err(_) => return "Usuário".to_string(), // entrada fechada
            Ok(_) => {}
        }

        let name = input.trim().to_string();
        if !name.is_empty() {
            return name;
        }
        println!("O nome não pode ficar vazio.");
    }
}

// Use esta função onde o app precisa do nome
pub fn get_or_ask_username() -> String {
    if let Some(settings) = load() {
        return settings.username;
    }

    let username = ask_username();
    let settings = Settings { username: username.clone() };
    if let Err(e) = save(&settings) {
        eprintln!("Não foi possível salvar as configurações: {e}");
    }
    username
}
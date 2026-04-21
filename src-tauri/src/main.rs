// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Server {
    id: i64,
    name: String,
    #[serde(rename = "ssUrl")]
    ss_url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct User {
    name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ServersResponse {
    user: User,
    servers: Vec<Server>,
}

// Reads the user token embedded in the executable filename. The biolink
// download endpoint streams the generic binary with a tokenized filename
// like "MoneyMakersVPN_<token>.exe". If the user hasn't renamed the file,
// we read it here so the app is automatically "signed in" on first run.
// Returns None if the filename doesn't contain a token (generic unpersonalized
// download, or user renamed the file).
#[tauri::command]
fn get_embedded_token() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_string_lossy().to_string();
    let prefix = "MoneyMakersVPN_";
    if let Some(rest) = stem.strip_prefix(prefix) {
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}

#[tauri::command]
async fn fetch_servers(base_url: String, token: String) -> Result<ServersResponse, String> {
    let url = format!(
        "{}/api/vpn/app/{}/servers",
        base_url.trim_end_matches('/'),
        token
    );
    let res = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("http {}", res.status()));
    }
    res.json::<ServersResponse>()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ping(base_url: String, token: String) -> Result<(), String> {
    let url = format!(
        "{}/api/vpn/app/{}/ping",
        base_url.trim_end_matches('/'),
        token
    );
    let _ = reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Placeholder — milestone 2 will wire this up to sing-box with TUN mode.
#[tauri::command]
async fn connect(_server_id: i64) -> Result<(), String> {
    Err("connect not implemented yet (milestone 2)".into())
}

#[tauri::command]
async fn disconnect() -> Result<(), String> {
    Err("disconnect not implemented yet (milestone 2)".into())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_embedded_token,
            fetch_servers,
            ping,
            connect,
            disconnect
        ])
        .run(tauri::generate_context!())
        .expect("error while running MoneyMakers VPN");
}

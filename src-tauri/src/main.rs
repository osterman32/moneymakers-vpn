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

#[derive(Deserialize, Debug)]
struct RegisterResponse {
    token: String,
}

// Reads the invite code embedded in the executable filename. The download
// endpoint on biolink streams the generic binary with a tokenized filename
// like "MoneyMakersVPN_<code>.exe". If the user hasn't renamed the file, we
// read it here so the first-run register form can auto-fill the code field.
#[tauri::command]
fn get_invite_code() -> Option<String> {
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
fn get_device_os() -> String {
    if cfg!(target_os = "windows") {
        "Windows".to_string()
    } else if cfg!(target_os = "macos") {
        "macOS".to_string()
    } else if cfg!(target_os = "linux") {
        "Linux".to_string()
    } else {
        "Unknown".to_string()
    }
}

#[tauri::command]
async fn register(
    base_url: String,
    code: String,
    name: String,
    device_os: String,
) -> Result<String, String> {
    let url = format!(
        "{}/api/vpn/app/register",
        base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "code": code,
        "name": name,
        "deviceOs": device_os,
    });
    let res = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let code = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("register failed ({}): {}", code, text));
    }
    let r: RegisterResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(r.token)
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

// Placeholder — milestone 2 will wire this up to sing-box with TUN mode,
// elevate via UAC (Windows) / authorization prompt (macOS), and actually
// route system traffic through the selected server.
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
            get_invite_code,
            get_device_os,
            register,
            fetch_servers,
            ping,
            connect,
            disconnect
        ])
        .run(tauri::generate_context!())
        .expect("error while running MoneyMakers VPN");
}

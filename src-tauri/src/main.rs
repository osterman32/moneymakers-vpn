// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Mutex;
use tauri::{Manager, State};
use tauri_plugin_shell::{process::CommandChild, ShellExt};

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

// Holds the currently-running sing-box child so Disconnect can kill it.
#[derive(Default)]
struct ConnectionState {
    child: Option<CommandChild>,
}

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

// ss://BASE64(method:password)@host:port/?outline=1  or  BASE64URL, with/without padding
fn parse_ss_url(url: &str) -> Result<(String, String, String, u16), String> {
    let s = url.strip_prefix("ss://").ok_or("not an ss:// URL")?;
    let s = s.split('#').next().unwrap();
    let s = s.split('?').next().unwrap();
    let s = s.trim_end_matches('/');
    let (userinfo, hostport) = s.rsplit_once('@').ok_or("missing @ in ss:// URL")?;

    let decoded = general_purpose::STANDARD
        .decode(userinfo)
        .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(userinfo))
        .or_else(|_| general_purpose::URL_SAFE.decode(userinfo))
        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(userinfo))
        .map_err(|e| format!("base64 decode: {}", e))?;
    let decoded_str =
        std::str::from_utf8(&decoded).map_err(|_| "invalid utf-8 in userinfo")?;
    let (method, password) = decoded_str
        .split_once(':')
        .ok_or("no ':' in decoded userinfo")?;

    let (host, port_str) = hostport.rsplit_once(':').ok_or("no ':' in hostport")?;
    let port: u16 = port_str.parse().map_err(|_| "bad port number")?;

    Ok((
        method.to_string(),
        password.to_string(),
        host.to_string(),
        port,
    ))
}

fn build_singbox_config(ss_url: &str, log_path: &str) -> Result<String, String> {
    let (method, password, host, port) = parse_ss_url(ss_url)?;
    let cfg = serde_json::json!({
        "log": {
            "level": "info",
            "output": log_path,
            "timestamp": true
        },
        "inbounds": [{
            "type": "mixed",
            "tag": "proxy-in",
            "listen": "127.0.0.1",
            "listen_port": 10808,
            "sniff": true
        }],
        "outbounds": [
            {
                "type": "shadowsocks",
                "tag": "proxy-out",
                "server": host,
                "server_port": port,
                "method": method,
                "password": password
            },
            { "type": "direct", "tag": "direct" },
            { "type": "block",  "tag": "block" }
        ],
        "route": {
            "rules": [
                { "inbound": ["proxy-in"], "outbound": "proxy-out" }
            ],
            "final": "proxy-out"
        }
    });
    serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect(
    app: tauri::AppHandle,
    state: State<'_, Mutex<ConnectionState>>,
    ss_url: String,
) -> Result<(), String> {
    // stop any existing connection
    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        if let Some(child) = guard.child.take() {
            let _ = child.kill();
        }
    }

    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let config_path = config_dir.join("singbox.json");
    let log_path = config_dir.join("singbox.log");

    let config = build_singbox_config(&ss_url, &log_path.to_string_lossy())?;
    fs::write(&config_path, &config).map_err(|e| e.to_string())?;

    let cmd = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| format!("sidecar lookup: {}", e))?
        .args([
            "run".to_string(),
            "-c".to_string(),
            config_path.to_string_lossy().to_string(),
        ]);
    let (_rx, child) = cmd.spawn().map_err(|e| format!("spawn sing-box: {}", e))?;

    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.child = Some(child);
    }

    set_system_proxy(true).map_err(|e| format!("set proxy: {}", e))?;
    Ok(())
}

// Returns the last ~8KB of the sing-box log, for surfacing in the UI when
// something seems broken. Path mirrors the one in `build_singbox_config`.
#[tauri::command]
async fn read_log(app: tauri::AppHandle) -> Result<String, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let log_path = config_dir.join("singbox.log");
    let full = fs::read_to_string(&log_path).unwrap_or_else(|_| String::from("(no log yet)"));
    const MAX: usize = 8 * 1024;
    if full.len() <= MAX {
        Ok(full)
    } else {
        Ok(format!("…\n{}", &full[full.len() - MAX..]))
    }
}

#[tauri::command]
async fn disconnect(state: State<'_, Mutex<ConnectionState>>) -> Result<(), String> {
    let _ = set_system_proxy(false);
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    if let Some(child) = guard.child.take() {
        let _ = child.kill();
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_system_proxy(enable: bool) -> Result<(), String> {
    use std::process::Command;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    if enable {
        let out = Command::new("reg")
            .args([
                "add", key, "/v", "ProxyServer", "/t", "REG_SZ", "/d",
                "127.0.0.1:10808", "/f",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "reg ProxyServer: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let out = Command::new("reg")
            .args([
                "add", key, "/v", "ProxyOverride", "/t", "REG_SZ", "/d",
                "<local>", "/f",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "reg ProxyOverride: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        let out = Command::new("reg")
            .args([
                "add", key, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "1", "/f",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "reg ProxyEnable: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    } else {
        let _ = Command::new("reg")
            .args([
                "add", key, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f",
            ])
            .output();
    }
    notify_proxy_change();
    Ok(())
}

// Browsers (Chrome/Edge) cache proxy settings per-process. Changing the
// registry silently doesn't get them to re-read — you have to broadcast
// INTERNET_OPTION_SETTINGS_CHANGED + INTERNET_OPTION_REFRESH via wininet.
#[cfg(target_os = "windows")]
fn notify_proxy_change() {
    use windows_sys::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    unsafe {
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

#[cfg(target_os = "macos")]
fn set_system_proxy(_enable: bool) -> Result<(), String> {
    // TODO: wire up `networksetup -setsocksfirewallproxy` via osascript with admin.
    // For now, surface an error instead of silently connecting with no routing.
    Err("mac system-proxy support ships in a later build".into())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn set_system_proxy(_enable: bool) -> Result<(), String> {
    Err("system proxy not supported on this OS".into())
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(ConnectionState::default()))
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_embedded_token,
            fetch_servers,
            ping,
            connect,
            disconnect,
            read_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running MoneyMakers VPN");
}

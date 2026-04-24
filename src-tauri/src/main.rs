// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Mutex;
use tauri::{Manager, State};
#[cfg(target_os = "windows")]
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

// Holds the currently-running connection. Windows runs sing-box as an
// unprivileged sidecar child; macOS runs it inside a root shell supervisor
// spawned via osascript-admin (see macos::spawn_supervisor). Closing the
// FIFO write end is the signal that tells the supervisor to shut down.
#[derive(Default)]
struct ConnectionState {
    #[cfg(target_os = "windows")]
    child: Option<CommandChild>,
    #[cfg(target_os = "macos")]
    fifo_write: Option<std::fs::File>,
    #[cfg(target_os = "macos")]
    osa_child: Option<tokio::process::Child>,
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

// Returns the raw JSON the server sends, so the UI can branch on flags like
// updateRequired, disabled, or updateAvailable without us having to model the
// full payload up front.
#[tauri::command]
async fn fetch_servers(
    base_url: String,
    token: String,
    version: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut url = format!(
        "{}/api/vpn/app/{}/servers",
        base_url.trim_end_matches('/'),
        token
    );
    if let Some(v) = version.as_ref().filter(|s| !s.is_empty()) {
        url.push_str(&format!("?version={}", urlencode(v)));
    }
    let res = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let status = res.status();
    let body = res.json::<serde_json::Value>().await.map_err(|e| e.to_string())?;
    // Server returns 403 for disabled accounts but with a useful body — surface
    // it as Ok so the UI can render the disabled message instead of an error.
    if !status.is_success() && status.as_u16() != 403 {
        return Err(format!("http {}: {}", status, body));
    }
    Ok(body)
}

#[tauri::command]
async fn ping(
    base_url: String,
    token: String,
    version: Option<String>,
) -> Result<(), String> {
    let mut url = format!(
        "{}/api/vpn/app/{}/ping",
        base_url.trim_end_matches('/'),
        token
    );
    if let Some(v) = version.as_ref().filter(|s| !s.is_empty()) {
        url.push_str(&format!("?version={}", urlencode(v)));
    }
    let _ = reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Tiny percent-encoder for safe-set chars (versions are like "0.1.0"). Keeps
// us off a url-encoding crate dep for one call site.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
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

// Windows: SOCKS+HTTP mixed inbound on localhost; the app sets system proxy
// so browsers pick it up. Only the apps that honor system proxy are tunneled.
#[cfg(target_os = "windows")]
fn build_mixed_config(ss_url: &str, log_path: &str) -> Result<String, String> {
    let (method, password, host, port) = parse_ss_url(ss_url)?;
    let cfg = serde_json::json!({
        "log": {
            "level": "info",
            "output": log_path,
            "timestamp": true
        },
        "dns": {
            "servers": [
                { "tag": "remote", "address": "https://1.1.1.1/dns-query", "detour": "proxy-out" }
            ],
            "final": "remote"
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

// macOS: TUN inbound. sing-box creates a utun interface (needs root), installs
// routes for 0.0.0.0/1 + 128.0.0.0/1 (auto_route) so every app's traffic goes
// through it regardless of proxy awareness. strict_route prevents leaks if
// the process dies — routes stay put until a reconnect or reboot.
#[cfg(target_os = "macos")]
fn build_tun_config(ss_url: &str, log_path: &str) -> Result<String, String> {
    let (method, password, host, port) = parse_ss_url(ss_url)?;
    let cfg = serde_json::json!({
        "log": {
            "level": "info",
            "output": log_path,
            "timestamp": true
        },
        "dns": {
            "servers": [
                { "tag": "remote", "address": "https://1.1.1.1/dns-query", "detour": "proxy-out" }
            ],
            "final": "remote"
        },
        "inbounds": [{
            "type": "tun",
            "tag": "tun-in",
            "interface_name": "utun223",
            "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
            "mtu": 9000,
            "auto_route": true,
            "strict_route": true,
            "stack": "system",
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
                { "inbound": ["tun-in"], "outbound": "proxy-out" }
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
    // Tear down any stale connection first.
    disconnect_inner(&state).await;

    #[cfg(target_os = "windows")]
    {
        let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
        fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
        let config_path = config_dir.join("singbox.json");
        let log_path = config_dir.join("singbox.log");

        let config = build_mixed_config(&ss_url, &log_path.to_string_lossy())?;
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
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let (fifo_write, osa_child) = macos::spawn_supervisor(&app, &ss_url).await?;
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.fifo_write = Some(fifo_write);
        guard.osa_child = Some(osa_child);
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (app, state, ss_url);
        Err("unsupported platform".into())
    }
}

// Returns the last ~8KB of the sing-box log, for surfacing in the UI when
// something seems broken. Path mirrors the one in the config builders.
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
    disconnect_inner(&state).await;
    Ok(())
}

// Shared teardown path. Never fails — best-effort cleanup so a stuck state
// can always be reset by clicking Connect again.
async fn disconnect_inner(state: &State<'_, Mutex<ConnectionState>>) {
    #[cfg(target_os = "windows")]
    {
        let _ = set_system_proxy(false);
        let child_opt = state
            .lock()
            .ok()
            .and_then(|mut g| g.child.take());
        if let Some(child) = child_opt {
            let _ = child.kill();
        }
    }
    #[cfg(target_os = "macos")]
    {
        let (fifo, osa) = {
            match state.lock() {
                Ok(mut g) => (g.fifo_write.take(), g.osa_child.take()),
                Err(_) => (None, None),
            }
        };
        // Closing the FIFO write end makes the root supervisor see EOF, kill
        // sing-box cleanly, and exit. osascript then returns.
        drop(fifo);
        if let Some(mut child) = osa {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await;
        }
    }
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
mod macos {
    use std::fs::OpenOptions;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command as StdCommand, Stdio};
    use tauri::Manager;

    // The supervisor is spawned once as root per connect. It runs sing-box,
    // then blocks reading from a FIFO we own. When the UI side drops its FIFO
    // write handle (on disconnect, app-quit, or crash), cat gets EOF, we TERM
    // sing-box, wait for it, and exit — which releases the osascript chain.
    //
    // Paths are baked in at write-time; the wrapper takes no CLI args. That
    // lets us run it via a single AppleScript `quoted form of` without having
    // to escape space-containing paths across the Rust → osascript → shell
    // boundary (the "Application Support" component bites you otherwise).
    fn render_wrapper(sb: &Path, cfg: &Path, log: &Path, fifo: &Path) -> String {
        // Two waiters, whichever fires first triggers cleanup:
        //   1. `wait "$SB_PID"` — sing-box exits on its own (crash, bad config).
        //   2. `cat "$FIFO"` returns — UI closed its write end (disconnect/quit).
        // The FIFO watcher signals the main script via `kill -TERM $$` so we
        // drop out of `wait` and land in cleanup.
        format!(
            r#"#!/bin/bash
SB={sb}
CFG={cfg}
LOG={log}
FIFO={fifo}
SB_PID=
WATCHER_PID=

cleanup() {{
  [ -n "$SB_PID" ] && kill -TERM "$SB_PID" 2>/dev/null
  [ -n "$WATCHER_PID" ] && kill -TERM "$WATCHER_PID" 2>/dev/null
  rm -f "$FIFO"
  exit 0
}}
trap cleanup TERM INT

"$SB" run -c "$CFG" > "$LOG" 2>&1 &
SB_PID=$!

(cat "$FIFO" > /dev/null 2>&1; kill -TERM $$) &
WATCHER_PID=$!

wait "$SB_PID"
cleanup
"#,
            sb = shell_single_quote(&sb.to_string_lossy()),
            cfg = shell_single_quote(&cfg.to_string_lossy()),
            log = shell_single_quote(&log.to_string_lossy()),
            fifo = shell_single_quote(&fifo.to_string_lossy()),
        )
    }

    fn shell_single_quote(s: &str) -> String {
        // 'foo' — with embedded single-quotes escaped as '\''.
        format!("'{}'", s.replace('\'', r"'\''"))
    }

    fn applescript_string(s: &str) -> String {
        // AppleScript literal: wrap in "..." and escape \ and ".
        format!(
            "\"{}\"",
            s.replace('\\', r"\\").replace('"', r#"\""#)
        )
    }

    // Sidecar binaries land in Contents/MacOS/sing-box in the bundled .app
    // (Tauri strips the target-triple suffix at bundle time). For `tauri dev`
    // the binary sits in target/debug/ with the triple suffix — try both.
    fn find_singbox() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let parent = exe.parent().ok_or("exe has no parent directory")?;
        for name in [
            "sing-box",
            "sing-box-aarch64-apple-darwin",
            "sing-box-x86_64-apple-darwin",
            "sing-box-universal-apple-darwin",
        ] {
            let candidate = parent.join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        Err(format!(
            "sing-box binary not found near {}",
            exe.display()
        ))
    }

    pub(crate) async fn spawn_supervisor(
        app: &tauri::AppHandle,
        ss_url: &str,
    ) -> Result<(std::fs::File, tokio::process::Child), String> {
        let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
        let config_path = config_dir.join("singbox.json");
        let log_path = config_dir.join("singbox.log");
        let wrapper_path = config_dir.join("connect-supervisor.sh");
        let fifo_path = config_dir.join("ctl.fifo");

        let singbox_path = find_singbox()?;
        let config = super::build_tun_config(ss_url, &log_path.to_string_lossy())?;
        std::fs::write(&config_path, &config).map_err(|e| e.to_string())?;

        let wrapper = render_wrapper(&singbox_path, &config_path, &log_path, &fifo_path);
        std::fs::write(&wrapper_path, &wrapper).map_err(|e| e.to_string())?;
        let mut perm = std::fs::metadata(&wrapper_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&wrapper_path, perm).map_err(|e| e.to_string())?;

        // Fresh FIFO. Removing any stale one from a prior crash is safe — if
        // there's still a supervisor holding the old path open it'll just
        // get EOF a moment earlier than planned.
        let _ = std::fs::remove_file(&fifo_path);
        let status = StdCommand::new("mkfifo")
            .arg(&fifo_path)
            .status()
            .map_err(|e| format!("mkfifo spawn: {}", e))?;
        if !status.success() {
            return Err("mkfifo failed".into());
        }

        // AppleScript: run the wrapper with no args. `quoted form of` handles
        // the space in "Application Support" without us reaching for another
        // layer of shell escaping.
        let osa_source = format!(
            "do shell script quoted form of {} with administrator privileges",
            applescript_string(&wrapper_path.to_string_lossy())
        );

        let mut osa_child = tokio::process::Command::new("osascript")
            .args(["-e", &osa_source])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false)
            .spawn()
            .map_err(|e| format!("osascript spawn: {}", e))?;

        // Race the wrapper's FIFO-reader against osascript itself. If the
        // user cancels the password prompt, osascript exits first with
        // non-zero status — we surface that as an error. If the wrapper
        // reaches its `cat "$FIFO"`, our write-side open unblocks and we
        // hand the handle to the caller.
        let fifo_for_open = fifo_path.clone();
        let open_fut =
            tokio::task::spawn_blocking(move || OpenOptions::new().write(true).open(&fifo_for_open));

        tokio::select! {
            os_res = osa_child.wait() => {
                let _ = std::fs::remove_file(&fifo_path);
                let msg = match os_res {
                    Ok(status) if status.code() == Some(1) => {
                        "password prompt was cancelled".to_string()
                    }
                    Ok(status) => format!(
                        "osascript exited before supervisor started (status {})",
                        status.code().unwrap_or(-1)
                    ),
                    Err(e) => format!("osascript wait failed: {}", e),
                };
                Err(msg)
            }
            open_res = open_fut => {
                let file = open_res
                    .map_err(|e| format!("open task join: {}", e))?
                    .map_err(|e| format!("open fifo for write: {}", e))?;
                Ok((file, osa_child))
            }
        }
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(ConnectionState::default()))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            get_embedded_token,
            fetch_servers,
            ping,
            connect,
            disconnect,
            read_log
        ])
        .on_window_event(|_window, _event| {
            // Windows: clear the system proxy + kill sing-box when the main
            // window closes, or the user ends up stuck on a dead proxy with
            // no UI to recover from it.
            // macOS: nothing to do. Process exit (Cmd-Q / force-quit) closes
            // our FIFO write end, which is the supervisor's shutdown signal.
            #[cfg(target_os = "windows")]
            if let tauri::WindowEvent::CloseRequested { .. } = _event {
                let state: tauri::State<'_, Mutex<ConnectionState>> = _window.state();
                let _ = set_system_proxy(false);
                let child_opt = state.lock().ok().and_then(|mut g| g.child.take());
                if let Some(child) = child_opt {
                    let _ = child.kill();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running MoneyMakers VPN");
}

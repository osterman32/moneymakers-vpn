const { invoke } = window.__TAURI__.core;
const { open: openExternal } = window.__TAURI__.shell || {};
const updater = window.__TAURI__.updater || {};
const proc = window.__TAURI__.process || {};

const APP_VERSION = "0.2.0";
const BASE_URL = "https://moneymakers.inc";
const TOKEN_KEY = "mm_vpn_token";
const PING_INTERVAL_MS = 60_000;

let currentServers = [];
let connected = false;
let pingTimer = null;
let blocked = false;
const IS_MAC = /mac/i.test(navigator.platform || "") || /mac/i.test(navigator.userAgent || "");

function $(id) {
  return document.getElementById(id);
}

function setStatus(msg, kind) {
  const el = $("status");
  el.textContent = msg || "";
  el.className = "status" + (kind ? " " + kind : "");
}

function show(id) {
  for (const v of document.querySelectorAll(".view")) v.classList.add("hidden");
  $(id).classList.remove("hidden");
}

async function fetchServers(token) {
  return invoke("fetch_servers", {
    baseUrl: BASE_URL,
    token,
    version: APP_VERSION,
  });
}

async function pingServer(token) {
  try {
    await invoke("ping", { baseUrl: BASE_URL, token, version: APP_VERSION });
  } catch {
    // best-effort heartbeat
  }
}

function startPinging(token) {
  stopPinging();
  pingTimer = setInterval(() => pingServer(token), PING_INTERVAL_MS);
}

function stopPinging() {
  if (pingTimer) {
    clearInterval(pingTimer);
    pingTimer = null;
  }
}

function showBlocker(title, message, downloadUrl) {
  blocked = true;
  stopPinging();
  $("blocker-title").textContent = title;
  $("blocker-message").textContent = message;
  const btn = $("blocker-action");
  if (downloadUrl) {
    btn.classList.remove("hidden");
    btn.textContent = "Open download page";
    btn.onclick = () => {
      if (openExternal) {
        openExternal(downloadUrl).catch(() => {});
      } else {
        // Fallback for older Tauri shell plugin layouts.
        window.open(downloadUrl, "_blank");
      }
    };
  } else {
    btn.classList.add("hidden");
  }
  $("blocker").classList.remove("hidden");
}

function showUpdateBanner(latestVersion) {
  const bar = $("update-banner");
  bar.textContent = `New version v${latestVersion} is available — open moneymakers.inc to download.`;
  bar.classList.remove("hidden");
}

function hideUpdateBanner() {
  $("update-banner").classList.add("hidden");
}

function setConnectHintVisible(visible) {
  const el = $("connect-hint");
  if (!el) return;
  if (IS_MAC && visible) el.classList.remove("hidden");
  else el.classList.add("hidden");
}

function renderMain(data) {
  show("main");
  hideUpdateBanner();
  currentServers = data.servers || [];
  connected = false;
  $("connect-btn").textContent = "Connect";
  setConnectHintVisible(true);
  $("username").textContent = data.user?.name || "";
  const select = $("server-select");
  select.innerHTML = "";
  if (!currentServers.length) {
    const opt = document.createElement("option");
    opt.textContent = "no servers available";
    opt.disabled = true;
    select.appendChild(opt);
    $("connect-btn").disabled = true;
    return;
  }
  for (const s of currentServers) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.name;
    select.appendChild(opt);
  }
  $("connect-btn").disabled = false;
  if (data.updateAvailable && data.latestVersion) {
    showUpdateBanner(data.latestVersion);
  }
}

// Returns true if the response was handled as a blocker (disabled / update
// required) and the caller should NOT proceed to renderMain.
function handleBlockingResponse(data) {
  if (data?.disabled) {
    showBlocker(
      "Account suspended",
      data.message || "Your account has been suspended. Contact the admin.",
      null,
    );
    return true;
  }
  if (data?.updateRequired) {
    showBlocker(
      "Update required",
      data.message || "A newer version is required.",
      data.downloadUrl || "https://moneymakers.inc/",
    );
    return true;
  }
  return false;
}

async function proceedWithToken(token) {
  setStatus("loading…");
  try {
    const data = await fetchServers(token);
    if (handleBlockingResponse(data)) {
      // Save the token so a future launch (after they update / contact admin)
      // doesn't lose them — they can still re-attempt sign-in.
      localStorage.setItem(TOKEN_KEY, token);
      setStatus("");
      return false;
    }
    localStorage.setItem(TOKEN_KEY, token);
    renderMain(data);
    pingServer(token);
    startPinging(token);
    setStatus("");
    return true;
  } catch (e) {
    setStatus("couldn't sign in: " + e, "error");
    return false;
  }
}

async function maybeAutoUpdate() {
  if (!updater.check) return; // updater plugin not available (dev mode etc.)
  let update;
  try {
    update = await updater.check();
  } catch {
    return; // offline, GitHub down, signature mismatch, etc. — ignore
  }
  if (!update?.available) return;
  const ok = confirm(
    `A new version (${update.version}) is available.\n\n` +
      `Current: ${APP_VERSION}\nDownloading and installing will restart the app.\n\nUpdate now?`,
  );
  if (!ok) return;
  setStatus("downloading update…");
  try {
    await update.downloadAndInstall();
    if (proc.relaunch) await proc.relaunch();
  } catch (e) {
    setStatus("update failed: " + e, "error");
  }
}

async function startup() {
  await maybeAutoUpdate();
  const embedded = await invoke("get_embedded_token").catch(() => null);
  if (embedded) {
    if (await proceedWithToken(embedded)) return;
    if (blocked) return;
  }
  const saved = localStorage.getItem(TOKEN_KEY);
  if (saved) {
    if (await proceedWithToken(saved)) return;
    if (blocked) return;
    localStorage.removeItem(TOKEN_KEY);
  }
  show("paste-key");
  setStatus("");
}

$("key-submit").addEventListener("click", async () => {
  const raw = $("key-input").value.trim();
  if (!raw) return setStatus("paste your sign-in key", "error");
  await proceedWithToken(raw);
});

$("key-input").addEventListener("keydown", (e) => {
  if (e.key === "Enter") $("key-submit").click();
});

$("signout-btn").addEventListener("click", async () => {
  if (!confirm("sign out on this device?")) return;
  stopPinging();
  try {
    if (connected) await invoke("disconnect");
  } catch {}
  localStorage.removeItem(TOKEN_KEY);
  connected = false;
  show("paste-key");
  $("key-input").value = "";
  setStatus("");
});

$("log-btn").addEventListener("click", async () => {
  const out = $("log-output");
  if (!out.classList.contains("hidden")) {
    out.classList.add("hidden");
    return;
  }
  try {
    const text = await invoke("read_log");
    out.textContent = text;
    out.classList.remove("hidden");
  } catch (e) {
    out.textContent = "couldn't read log: " + e;
    out.classList.remove("hidden");
  }
});

$("connect-btn").addEventListener("click", async () => {
  if (connected) {
    setStatus("disconnecting…");
    try {
      await invoke("disconnect");
      connected = false;
      $("connect-btn").textContent = "Connect";
      $("server-select").disabled = false;
      setConnectHintVisible(true);
      setStatus("disconnected");
    } catch (e) {
      setStatus("disconnect failed: " + e, "error");
    }
    return;
  }
  const id = parseInt($("server-select").value, 10);
  const server = currentServers.find((s) => s.id === id);
  if (!server) return setStatus("pick a server", "error");
  setStatus(IS_MAC ? "connecting… (approve the password prompt)" : "connecting…");
  try {
    await invoke("connect", { ssUrl: server.ssUrl });
    connected = true;
    $("connect-btn").textContent = "Disconnect";
    $("server-select").disabled = true;
    setConnectHintVisible(false);
    setStatus(`connected via ${server.name}`, "ok");
  } catch (e) {
    setStatus("connect failed: " + e, "error");
  }
});

startup();

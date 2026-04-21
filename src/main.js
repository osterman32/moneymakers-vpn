const { invoke } = window.__TAURI__.core;

const BASE_URL = "https://moneymakers.inc";
const TOKEN_KEY = "mm_vpn_token";

let currentServers = [];
let connected = false;

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
  return invoke("fetch_servers", { baseUrl: BASE_URL, token });
}

async function pingServer(token) {
  try {
    await invoke("ping", { baseUrl: BASE_URL, token });
  } catch {
    // best-effort heartbeat
  }
}

function renderMain(data) {
  show("main");
  currentServers = data.servers;
  connected = false;
  $("connect-btn").textContent = "Connect";
  $("username").textContent = data.user.name;
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
}

async function proceedWithToken(token) {
  setStatus("loading…");
  try {
    const data = await fetchServers(token);
    localStorage.setItem(TOKEN_KEY, token);
    renderMain(data);
    pingServer(token);
    setStatus("");
    return true;
  } catch (e) {
    setStatus("couldn't sign in: " + e, "error");
    return false;
  }
}

async function startup() {
  const embedded = await invoke("get_embedded_token").catch(() => null);
  if (embedded) {
    if (await proceedWithToken(embedded)) return;
  }
  const saved = localStorage.getItem(TOKEN_KEY);
  if (saved) {
    if (await proceedWithToken(saved)) return;
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
      setStatus("disconnected");
    } catch (e) {
      setStatus("disconnect failed: " + e, "error");
    }
    return;
  }
  const id = parseInt($("server-select").value, 10);
  const server = currentServers.find((s) => s.id === id);
  if (!server) return setStatus("pick a server", "error");
  setStatus("connecting…");
  try {
    await invoke("connect", { ssUrl: server.ssUrl });
    connected = true;
    $("connect-btn").textContent = "Disconnect";
    $("server-select").disabled = true;
    setStatus(`connected via ${server.name}`, "ok");
  } catch (e) {
    setStatus("connect failed: " + e, "error");
  }
});

startup();

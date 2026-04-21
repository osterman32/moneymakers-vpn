const { invoke } = window.__TAURI__.core;

const BASE_URL = "https://moneymakers.inc";
const TOKEN_KEY = "mm_vpn_token";

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
  $("username").textContent = data.user.name;
  const select = $("server-select");
  select.innerHTML = "";
  if (!data.servers.length) {
    const opt = document.createElement("option");
    opt.textContent = "no servers available";
    opt.disabled = true;
    select.appendChild(opt);
    $("connect-btn").disabled = true;
    return;
  }
  for (const s of data.servers) {
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
  // 1. filename-embedded token (portable exe path)
  const embedded = await invoke("get_embedded_token").catch(() => null);
  if (embedded) {
    if (await proceedWithToken(embedded)) return;
  }
  // 2. previously-saved token (any prior successful sign-in)
  const saved = localStorage.getItem(TOKEN_KEY);
  if (saved) {
    if (await proceedWithToken(saved)) return;
    localStorage.removeItem(TOKEN_KEY);
  }
  // 3. manual paste fallback
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

$("signout-btn").addEventListener("click", () => {
  if (!confirm("sign out on this device?")) return;
  localStorage.removeItem(TOKEN_KEY);
  show("paste-key");
  $("key-input").value = "";
  setStatus("");
});

$("connect-btn").addEventListener("click", async () => {
  const id = parseInt($("server-select").value, 10);
  setStatus("connecting…");
  try {
    await invoke("connect", { serverId: id });
    setStatus("connected", "ok");
  } catch (e) {
    setStatus(String(e), "error");
  }
});

startup();

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

async function goMain(token) {
  setStatus("loading…");
  try {
    const data = await fetchServers(token);
    renderMain(data);
    pingServer(token);
    setStatus("");
  } catch (e) {
    // token invalid or server down; drop token and fall back to register
    localStorage.removeItem(TOKEN_KEY);
    setStatus("your access was revoked or the server is down: " + e, "error");
    await startup();
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

async function startup() {
  const saved = localStorage.getItem(TOKEN_KEY);
  if (saved) {
    return goMain(saved);
  }

  // No saved token — registration. Pre-fill the code from the exe filename
  // when we can; hide the code field when found so friends aren't confused.
  const embeddedCode = await invoke("get_invite_code").catch(() => null);
  if (embeddedCode) {
    $("code-input").value = embeddedCode;
    $("code-field").classList.add("hidden");
  } else {
    $("code-field").classList.remove("hidden");
  }
  show("register");
  setStatus("");
}

$("register-btn").addEventListener("click", async () => {
  const name = $("name-input").value.trim();
  const code = $("code-input").value.trim();
  if (!name) return setStatus("enter your name", "error");
  if (!code) return setStatus("missing invite code — ask Zain", "error");
  setStatus("joining…");
  try {
    const deviceOs = await invoke("get_device_os");
    const token = await invoke("register", {
      baseUrl: BASE_URL,
      code,
      name,
      deviceOs,
    });
    localStorage.setItem(TOKEN_KEY, token);
    await goMain(token);
  } catch (e) {
    setStatus(String(e), "error");
  }
});

$("reset-btn").addEventListener("click", () => {
  if (!confirm("sign out? you'll have to re-join with the invite code.")) return;
  localStorage.removeItem(TOKEN_KEY);
  startup();
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

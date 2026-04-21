const { invoke } = window.__TAURI__.core;

const BASE_URL = "https://moneymakers.inc";

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

async function startup() {
  const token = await invoke("get_embedded_token").catch(() => null);
  if (!token) {
    show("needs-download");
    return;
  }
  setStatus("loading…");
  try {
    const data = await fetchServers(token);
    renderMain(data);
    pingServer(token);
    setStatus("");
  } catch (e) {
    setStatus(
      "couldn't reach the server or your access was revoked: " + e,
      "error",
    );
    show("needs-download");
  }
}

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

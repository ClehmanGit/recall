const $ = (id) => document.getElementById(id);

function ago(ms) {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  return `${Math.round(s / 3600)}h ago`;
}

async function render() {
  const st = await chrome.runtime.sendMessage({ type: "status" });
  $("app").textContent = st.app ? "connected" : "not running";
  $("app").className = st.app ? "ok" : "bad";
  const errs = [];
  for (const p of ["claude", "gemini", "chatgpt"]) {
    const l = st.last[p];
    if (!l) { $(p).textContent = "never"; continue; }
    if (l.error) { $(p).textContent = "error"; $(p).className = "bad"; errs.push(`${p}: ${l.error}`); continue; }
    $(p).textContent = `${l.sent} sent · ${ago(l.at)}`;
    $(p).className = "";
  }
  $("err").textContent = errs.join("\n");
  const { token } = await chrome.storage.local.get("token");
  if (token) $("token").placeholder = "saved";
}

$("save").addEventListener("click", async () => {
  const token = $("token").value.trim();
  if (token) await chrome.storage.local.set({ token });
  $("token").value = "";
  render();
});

$("sync").addEventListener("click", async () => {
  $("sync").textContent = "Syncing…";
  await chrome.runtime.sendMessage({ type: "sync" });
  $("sync").textContent = "Sync now";
  render();
});

render();

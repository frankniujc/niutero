import { extractCitation } from "./lib/extract.js";
import { getConfig } from "./lib/config.js";
import { health } from "./lib/connector.js";

const $ = (id) => document.getElementById(id);

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

async function init() {
  $("opts").addEventListener("click", (e) => {
    e.preventDefault();
    chrome.runtime.openOptionsPage();
  });
  $("capture").addEventListener("click", onCapture);

  const cfg = await getConfig();

  // Connector reachability dot (non-blocking).
  health(cfg).then((up) => {
    const dot = $("conn");
    dot.classList.toggle("up", up);
    dot.classList.toggle("down", !up);
    dot.title = up
      ? `connector up on 127.0.0.1:${cfg.port}`
      : `connector not reachable on 127.0.0.1:${cfg.port}`;
  });

  if (!cfg.token) {
    $("detect").innerHTML =
      'No session token set. <a id="go" href="#">Open options</a> and paste the token printed by <code>niutero-cli connector</code>.';
    const go = $("go");
    if (go)
      go.addEventListener("click", (e) => {
        e.preventDefault();
        chrome.runtime.openOptionsPage();
      });
    return;
  }

  // Preview what we'd capture.
  const tab = await activeTab();
  if (!tab || !/^https?:/i.test(tab.url || "")) {
    $("detect").textContent = "This isn't a normal web page.";
    return;
  }
  try {
    const [inj] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: extractCitation,
    });
    renderDetect(inj && inj.result);
  } catch {
    $("detect").textContent = "Can't read this page.";
  }
}

function renderDetect(d) {
  const el = $("detect");
  el.textContent = "";
  if (!d || (!d.doi && !d.hasMeta)) {
    el.textContent = "No citation found on this page.";
    $("capture").disabled = true;
    return;
  }
  const title = (d.fields && d.fields.title) || d.pageTitle || "(untitled)";
  const via = d.doi ? `DOI ${d.doi}` : "page metadata";
  const t = document.createElement("div");
  t.className = "title";
  t.textContent = title;
  const s = document.createElement("div");
  s.className = "sub";
  s.textContent = "via " + via;
  el.append(t, s);
  $("capture").disabled = false;
}

async function onCapture() {
  const btn = $("capture");
  btn.disabled = true;
  btn.textContent = "Capturing…";
  let result;
  try {
    result = await chrome.runtime.sendMessage({ type: "capture" });
  } catch (e) {
    result = { ok: false, error: "The extension worker did not respond." };
  }
  showStatus(result);
  btn.textContent = "Capture citation";
  btn.disabled = false;
}

function showStatus(r) {
  const st = $("status");
  st.hidden = false;
  if (!r) {
    st.className = "status bad";
    st.textContent = "No response.";
    return;
  }
  if (r.ok) {
    if (r.added) {
      st.className = "status good";
      st.textContent = `Added to your library${r.via === "doi" ? "" : " (from page tags)"}.`;
    } else {
      st.className = "status dup";
      st.textContent = "Already in your library.";
    }
  } else {
    st.className = "status bad";
    st.textContent = r.error || "Capture failed.";
  }
}

init();

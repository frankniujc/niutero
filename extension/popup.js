"use strict";

// Single source of truth for the endpoint. Must match niutero-server's
// DEFAULT_PORT (the Rust `connector::DEFAULT_PORT`) and the manifest
// "host_permissions". Change all three together if you change the port.
const NIUTERO = "http://127.0.0.1:23510";

// Firefox exposes promise-based `browser.*`; Chrome uses `chrome.*` (also
// promise-based for tabs/scripting in MV3). This works on both.
const api = typeof browser !== "undefined" ? browser : chrome;

const $ = (id) => document.getElementById(id);
let scraped = null;

function setStatus(id, text, cls) {
  const el = $(id);
  el.textContent = text;
  el.className = "status " + (cls || "muted");
}

async function ping() {
  try {
    const r = await fetch(NIUTERO + "/ping", { method: "GET" });
    const j = await r.json();
    if (j && j.ok) {
      setStatus(
        "conn",
        j.library ? 'Connected — library "' + j.library + '"' : "Connected — no library open",
        j.library ? "ok" : "bad",
      );
      return Boolean(j.library);
    }
  } catch {
    /* fall through */
  }
  setStatus(
    "conn",
    "niutero is not reachable. Open it and enable the connector in Settings → Sync & sharing.",
    "bad",
  );
  return false;
}

async function scrapeActiveTab() {
  const tabs = await api.tabs.query({ active: true, currentWindow: true });
  const tab = tabs && tabs[0];
  if (!tab || !tab.id) throw new Error("no active tab");
  const res = await api.scripting.executeScript({
    target: { tabId: tab.id },
    files: ["scrape.js"],
  });
  return (res && res[0] && res[0].result) || { metadata: {} };
}

function describe(s) {
  const m = s.metadata || {};
  if (m.title) {
    return { title: m.title, sub: s.identifier || (m.authors && m.authors[0]) || "" };
  }
  if (s.identifier) return { title: s.identifier, sub: "" };
  return null;
}

async function save() {
  if (!scraped) return;
  $("save").disabled = true;
  setStatus("result", "Saving…", "muted");
  const tags = $("tags")
    .value.split(",")
    .map((t) => t.trim())
    .filter(Boolean);
  const body = { metadata: scraped.metadata || {}, tags };
  if (scraped.identifier) body.identifier = scraped.identifier;
  try {
    const r = await fetch(NIUTERO + "/import", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const j = await r.json();
    if (j && j.ok) {
      if (j.added > 0) {
        setStatus("result", "Saved: " + (j.title || j.citekey || "done"), "ok");
      } else {
        setStatus("result", "Already in your library", "muted");
      }
    } else {
      setStatus("result", "Not saved: " + ((j && j.error) || "unknown error"), "bad");
      $("save").disabled = false;
    }
  } catch (e) {
    setStatus("result", "Request failed: " + e.message, "bad");
    $("save").disabled = false;
  }
}

(async () => {
  const reachable = await ping();
  try {
    scraped = await scrapeActiveTab();
  } catch (e) {
    $("detected").textContent = "Cannot read this page (" + e.message + ").";
    return;
  }
  const d = describe(scraped);
  if (d) {
    $("detected").textContent = "";
    const t = document.createElement("div");
    t.className = "title";
    t.textContent = d.title;
    $("detected").appendChild(t);
    if (d.sub) {
      const s = document.createElement("div");
      s.className = "sub";
      s.textContent = d.sub;
      $("detected").appendChild(s);
    }
    $("save").disabled = !reachable;
  } else {
    $("detected").textContent = "No DOI, arXiv id, or citation metadata found on this page.";
  }
  $("save").addEventListener("click", save);
})();

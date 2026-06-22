// Service worker: wires the non-popup entry points (keyboard command, context
// menu) and answers the popup's capture request. The capture flow itself lives
// in lib/capture.js so every entry point behaves identically.

import { captureTab } from "./lib/capture.js";

const MENU_ID = "niutero-capture";

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create(
    {
      id: MENU_ID,
      title: "Capture citation to niutero",
      contexts: ["page", "selection", "link"],
    },
    () => void chrome.runtime.lastError,
  );
});

// The popup drives capture by message so the flow stays in one place.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg && msg.type === "capture") {
    captureActive().then(sendResponse);
    return true; // keep the channel open for the async response
  }
  return false;
});

chrome.commands.onCommand.addListener((cmd) => {
  if (cmd === "capture") captureActiveAndNotify();
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === MENU_ID) captureActiveAndNotify(tab);
});

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

async function captureActive() {
  return captureTab(await activeTab());
}

async function captureActiveAndNotify(tab) {
  const result = await captureTab(tab || (await activeTab()));
  notify(result);
  badge(result);
}

function notify(r) {
  const title = r.ok
    ? r.added
      ? "Captured to niutero"
      : "Already in your library"
    : "niutero capture failed";
  const subject = r.title || r.doi || (r.added ? "entry" : "this entry");
  const message = r.ok
    ? r.added
      ? `Added ${subject}${r.via === "doi" ? "" : " (from page tags)"}.`
      : `${subject} was already saved.`
    : r.error;
  chrome.notifications.create(
    {
      type: "basic",
      iconUrl: chrome.runtime.getURL("icons/icon-128.png"),
      title,
      message: message || "",
    },
    () => void chrome.runtime.lastError,
  );
}

function badge(r) {
  const text = r.ok ? (r.added ? "✓" : "–") : "!";
  const color = r.ok ? (r.added ? "#1F8A5B" : "#9AA0A6") : "#C0392B";
  chrome.action.setBadgeText({ text });
  chrome.action.setBadgeBackgroundColor({ color });
  // Clear after a moment. If the worker sleeps first the badge simply persists
  // until the next capture resets it — harmless.
  setTimeout(() => chrome.action.setBadgeText({ text: "" }), 4000);
}

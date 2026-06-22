// Extension settings, stored per-machine in chrome.storage.local. The token is
// the session token the connector printed (`niutero-cli connector <vault>`);
// it never leaves this machine and is only ever sent to 127.0.0.1.

export const DEFAULT_PORT = 23510;

export async function getConfig() {
  const d = await chrome.storage.local.get({ port: DEFAULT_PORT, token: "" });
  return {
    port: Number(d.port) || DEFAULT_PORT,
    token: (d.token || "").trim(),
  };
}

export async function setConfig(cfg) {
  await chrome.storage.local.set({
    port: Number(cfg.port) || DEFAULT_PORT,
    token: (cfg.token || "").trim(),
  });
}

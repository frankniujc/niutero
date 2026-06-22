// Talks to the local niutero connector (the loopback HTTP server started by
// `niutero-cli connector <vault>`). Two mutating routes, both token-gated:
//   POST /capture      body = BibTeX        (built from the page's meta tags)
//   POST /capture/doi  body = a bare DOI    (resolved to canonical BibTeX server-side)
// Both answer { "added": N, "skipped": M }. A 4xx carries { "error": "..." }.

function base(cfg) {
  return `http://127.0.0.1:${cfg.port}`;
}

async function post(cfg, path, body, timeoutMs) {
  if (!cfg.token) {
    throw new Error(
      "No session token set. Open the extension options and paste the token printed by `niutero-cli connector`.",
    );
  }
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), timeoutMs);
  let resp;
  try {
    resp = await fetch(base(cfg) + path, {
      method: "POST",
      headers: {
        "Content-Type": "text/plain; charset=utf-8",
        Authorization: "Bearer " + cfg.token,
      },
      body,
      signal: ctrl.signal,
    });
  } catch (e) {
    throw new Error(unreachable(e, cfg));
  } finally {
    clearTimeout(timer);
  }

  const text = await resp.text();
  let data = {};
  try {
    data = JSON.parse(text);
  } catch {
    // non-JSON body; fall through to the status-based message
  }
  if (!resp.ok) {
    if (resp.status === 401) {
      throw new Error(
        "The connector rejected the token (401). Re-copy it from the terminal into options.",
      );
    }
    throw new Error(data.error || `connector error (HTTP ${resp.status})`);
  }
  return { added: data.added || 0, skipped: data.skipped || 0 };
}

export function sendBibtex(cfg, bibtex) {
  return post(cfg, "/capture", bibtex, 20000);
}

// The DOI route resolves over the network on the server, so it gets a longer
// budget than a local BibTeX merge.
export function sendDoi(cfg, doi) {
  return post(cfg, "/capture/doi", doi, 40000);
}

export async function health(cfg) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 5000);
  try {
    const resp = await fetch(base(cfg) + "/health", { signal: ctrl.signal });
    const text = await resp.text();
    return resp.ok && text.includes("niutero-connector");
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

function unreachable(e, cfg) {
  if (e && e.name === "AbortError") {
    return "The connector did not respond in time. Is it still running?";
  }
  return `Could not reach the connector on 127.0.0.1:${cfg.port}. Start it with \`niutero-cli connector <vault>\` and check the port in options.`;
}

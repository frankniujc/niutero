# niutero browser connector (Chrome extension)

A Manifest V3 extension that captures the citation on the page you're viewing
and sends it to your **local** niutero library, through the connector server
built into `niutero-cli`. It talks to nothing but `127.0.0.1` — there is no
cloud service and no telemetry.

## How it works

```
 web page ──(read meta tags)──> extension ──POST──> 127.0.0.1:<port> ──> your vault
                                    │                 (niutero-cli connector)
                          DOI?  ────┘
```

The extension extracts what it can from the page:

- **If the page has a DOI** (most publishers, ACL Anthology, PubMed, Google
  Scholar; arXiv via its DataCite DOI) it sends the bare DOI to the connector's
  `POST /capture/doi` route, and the **server** resolves it to canonical BibTeX
  via doi.org. This is why the extension needs no network permission beyond the
  loopback connector.
- **Otherwise** it builds a BibTeX entry from the page's Highwire Press
  `citation_*` / Dublin Core meta tags and sends that to `POST /capture`.

Either way the connector merges with a skip-on-duplicate policy and runs your
vault's opt-in import hooks (PDF auto-fetch, enrich, auto-commit).

## Install (unpacked)

1. Start the connector and copy the session token it prints:

   ```sh
   cargo run -p niutero-cli -- connector /path/to/vault
   # Browser connector listening on http://127.0.0.1:23510  (Ctrl-C to stop)
   #   POST BibTeX to /capture, or a bare DOI to /capture/doi
   # session token: 1a2b3c…
   ```

2. In Chrome, open `chrome://extensions`, enable **Developer mode**, click
   **Load unpacked**, and select this `extension/` folder.

3. Click the niutero toolbar icon → **Options**, paste the token, confirm the
   port matches (default **23510**), and hit **Test connection**.

## Use

On a paper's page, capture via any of:

- the toolbar button (shows what it detected, then a **Capture** button),
- the keyboard shortcut **Alt+Shift+S**,
- right-click → **Capture citation to niutero**.

The popup shows the result inline. The keyboard and context-menu paths (which
have no popup) flash a toolbar badge — `✓` added, `–` already saved, `!` error —
and post a desktop notification.

## Security

- The session token is stored only in `chrome.storage.local` on this machine
  and is sent only to `127.0.0.1`. The connector rejects any request without it
  (loopback is not an authorization boundary — any local page can reach it).
- `host_permissions` is loopback-only. The extension never reads or sends your
  browsing anywhere else; DOI resolution happens server-side.
- Page access uses `activeTab` + `scripting`, so the content extractor runs only
  on the tab you explicitly capture, only when you trigger it.

## Develop

- `node --test` — unit tests for the BibTeX fallback builder (`lib/bibtex.js`).
- `python gen-icons.py` — regenerate `icons/*.png` (pure stdlib, no deps).

Files: `manifest.json`, `background.js` (service worker), `popup.*`,
`options.*`, and `lib/` (`extract.js` page scraper, `bibtex.js` builder,
`connector.js` client, `config.js`, `capture.js` flow).

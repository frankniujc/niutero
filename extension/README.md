# niutero browser connector (Chrome / Firefox)

A small Manifest V3 extension: click the toolbar button on a paper's page and
its reference goes straight into the library you have **open in niutero**.
niutero resolves it to a *canonical* source over the network — an **OpenReview**
submission's venue BibTeX, or a DOI / arXiv id (the same path as its *Import by
DOI*) — and falls back to scraped Highwire / Dublin Core metadata only when the
page carries no identifier.

It talks to nothing but `127.0.0.1` — there is no cloud service and no telemetry.

## How it works

```
 web page ──(scrape)──> extension ──POST /import──> 127.0.0.1:23510 ──> open library
                                                     (niutero hosts the server)
```

The extension only reads the page: it returns `{ identifier, metadata }` and
lets **niutero** decide. On an OpenReview page it sends `openreview:<id>` and
niutero fetches the venue's official BibTeX from the OpenReview API; with a DOI
or arXiv id niutero fetches canonical BibTeX from doi.org; otherwise it builds an
entry from the scraped meta tags. Either way the entry is re-keyed to your
library's pattern, merged (skip-on-duplicate), **normalized**, and the rest of
your import hooks run (enrich / PDF-fetch / auto-commit, if enabled) before the
open library refreshes — so a capture lands as a clean, ready-to-use entry.

## Enable it in niutero first

Open niutero → **Settings → Sync & sharing → Browser connector** and turn on
*"Run a local capture server while niutero is open."* The status line should
read `Listening on 127.0.0.1:23510`. The server runs only while niutero is open
and imports into whichever library you have open.

(For a headless setup, `niutero-cli connector <vault>` hosts the same server
against one fixed vault.)

## Load the extension (unpacked)

**Chrome / Edge / Brave** — go to `chrome://extensions`, turn on **Developer
mode**, click **Load unpacked**, and select this `extension/` folder.

**Firefox** — go to `about:debugging#/runtime/this-firefox`, click **Load
Temporary Add-on…**, and select `extension/manifest.json`.

Then open a paper page, click **Save to niutero**, optionally add tags, and
press **Save**.

## Security (Zotero-Connector-style — no token, no pairing)

The server is hardened the way Zotero's connector is, because the real threats
to a localhost helper are DNS rebinding and web-page CSRF, not other local
processes:

- bound to **`127.0.0.1` only**;
- rejects any request whose **`Host`** is not loopback (anti DNS-rebinding);
- requires an **extension `Origin`** (`chrome-extension://` / `moz-extension://`);
  ordinary web origins are refused (anti CSRF);
- sends **no `Access-Control-Allow-*`**, so a web page's script can't read
  responses, while this extension's `host_permissions` fetch is unaffected.

There is intentionally no token and no pairing step. Page access uses
`activeTab` + `scripting`, so the scraper runs only on the tab you capture,
only when you click.

## Port

The endpoint is `http://127.0.0.1:23510`. It is referenced in three places that
must stay in sync if you change it: the Rust `connector::DEFAULT_PORT`,
`manifest.json` (`host_permissions`), and `popup.js` (`NIUTERO`).

//! Browser connector — a tiny loopback HTTP server that turns the page you're
//! viewing into a library entry. The browser-side client (a Manifest V3
//! extension) lives in `extension/` at the repo root; this is the endpoint it
//! talks to.
//!
//! **Hosting.** The server runs on a background thread and resolves every
//! capture against a host-updated [`ConnectorShared`] (the currently-open vault
//! root + library name). The GUI hosts it while it's open, so captures land in
//! the library you have open — no separate process, no path argument. The CLI
//! `connector` subcommand hosts the same server against one fixed vault.
//!
//! **Security (Zotero-Connector-style — no token, no pairing).** The real
//! threats to a localhost helper are DNS rebinding and web-page CSRF, not other
//! local processes, so instead of a token:
//! 1. bind **`127.0.0.1` only** (never `0.0.0.0`);
//! 2. require a **loopback `Host`** header (defeats DNS rebinding);
//! 3. `POST /import` requires a **present extension `Origin`**
//!    (`*-extension://`): ordinary web origins, a missing `Origin`, and the
//!    opaque `Origin: null` (what a sandboxed iframe or `file:` page sends) are
//!    all refused (defeats page CSRF). The read-only `GET /ping` tolerates a
//!    missing `Origin` so `curl` debugging works; it leaks nothing.
//! 4. send **no `Access-Control-Allow-*`** — a web page's script can't read our
//!    responses, while the extension's `host_permissions` fetch is unaffected.
//!
//! **Routes** (both JSON):
//! - `GET /ping` → `{app, ok, version, library}` so the extension can show
//!   whether niutero is up and which library is open.
//! - `POST /import` with `{identifier?, metadata?, tags?}` → resolve, merge with
//!   the library's dup policy, run the import hooks, and answer `{ok, citekey,
//!   …}`. Resolution prefers a canonical source: an OpenReview submission id
//!   (its venue BibTeX, via the OpenReview API), then a DOI / `arXiv:` id (via
//!   doi.org), and only falls back to the page's scraped metadata. Every
//!   resolved entry is re-keyed to the library's cite-key pattern and **always
//!   normalized** — the connector's job is to turn a page into a clean,
//!   ready-to-use entry, so it normalizes regardless of the `normalize_on_import`
//!   toggle (which governs bulk/CLI imports).
//!
//! Everything but the accept loop is pure and unit-tested; the loop itself is
//! covered by a loopback integration test.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;

use niutero_bib::{entries, parse};
use niutero_core::BibEntry;
use niutero_vault::Vault;

/// Default loopback port. Deliberately not Zotero's `23119`, so both can run
/// during a migration. The browser extension hardcodes the same value.
pub const DEFAULT_PORT: u16 = 23510;

/// Largest accepted request body (one entry's worth of metadata). Anything
/// larger gets `413` without allocation.
const MAX_BODY: usize = 64 * 1024;

/// Largest accepted request head (request line + headers) — a drip-fed header
/// stream can't grow memory or wedge the single-threaded accept loop.
const MAX_HEAD: usize = 16 * 1024;

/// Per-connection socket read/write timeout — a slow client can't wedge the loop.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the accept loop wakes to re-check the shutdown flag.
const ACCEPT_POLL: Duration = Duration::from_millis(400);

// ----------------------------------------------------------- protocol types

/// Page metadata scraped by the extension — the offline fallback used when a
/// capture carries no resolvable identifier. All fields optional.
#[derive(Debug, Default, Deserialize)]
pub struct ScrapedMetadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub year: String,
    #[serde(default)]
    pub journal: String,
    #[serde(default)]
    pub booktitle: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub volume: String,
    #[serde(default)]
    pub issue: String,
    #[serde(default)]
    pub pages: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub url: String,
    /// `"article"` / `"conference"` / … — a hint for the entry type.
    #[serde(default)]
    pub item_type: String,
}

/// One capture from the extension.
#[derive(Debug, Default, Deserialize)]
pub struct ImportRequest {
    /// A DOI or `arXiv:<id>` to resolve over the network (preferred).
    #[serde(default)]
    pub identifier: Option<String>,
    /// Scraped metadata, used when there's no identifier.
    #[serde(default)]
    pub metadata: Option<ScrapedMetadata>,
    /// Tags to attach in the sidecar.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The result of applying a capture — for the extension's toast and the log.
#[derive(Debug, Clone, Default)]
pub struct ImportOutcome {
    pub citekey: String,
    /// Display title — brace protection stripped for the popup toast; the
    /// stored entry keeps its `{{…}}`.
    pub title: String,
    pub added: usize,
    pub overwritten: usize,
    pub skipped: usize,
    /// Same-paper re-captures added under a new key (`on_dup = rename`).
    pub renamed: usize,
    /// The popup's tags were applied to at least one entry — including the
    /// stored twin of a skipped duplicate.
    pub tags_updated: bool,
    /// Set when the identifier couldn't be resolved and the entry was built
    /// from the page's scraped metadata instead (holds the reason).
    pub fallback: Option<String>,
}

// ------------------------------------------------------------- server config

/// Host-updated state the server reads on each request. The GUI rewrites this
/// every frame so captures always target the currently-open library; the CLI
/// sets it once.
#[derive(Default)]
pub struct ConnectorShared {
    /// Root of the library to import into, or `None` if none is open.
    pub vault_root: Option<PathBuf>,
    /// Name of that library, reported by `/ping`.
    pub library: Option<String>,
}

/// How to start the server.
pub struct ConnectorConfig {
    /// Loopback port to bind (usually [`DEFAULT_PORT`]); `0` binds an ephemeral
    /// port (tests).
    pub port: u16,
    /// Shared, host-updated target library (and `/ping` status).
    pub shared: Arc<Mutex<ConnectorShared>>,
    /// Called with the outcome after a successful import so a UI host can
    /// toast accurately and refresh only when something changed. No-op for CLI.
    pub on_import: Arc<dyn Fn(&ImportOutcome) + Send + Sync>,
}

/// A running server. Drop (or [`stop`](ServerHandle::stop)) shuts it down.
pub struct ServerHandle {
    port: u16,
    shutdown: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl ServerHandle {
    /// The actually-bound loopback port (resolved even when `cfg.port` was `0`).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop the server and join its thread.
    pub fn stop(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // Signal the accept loop; it exits within one ACCEPT_POLL tick. Don't
        // block the dropping thread on join.
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// Bind `127.0.0.1:cfg.port` and serve on a background thread.
pub fn start(cfg: ConnectorConfig) -> io::Result<ServerHandle> {
    let listener = TcpListener::bind(("127.0.0.1", cfg.port))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let shut = Arc::clone(&shutdown);
    let cfg = Arc::new(cfg);
    let join = std::thread::Builder::new()
        .name("niutero-connector".to_string())
        .spawn(move || serve_loop(listener, cfg, shut))?;
    log::info!("connector listening on 127.0.0.1:{port}");
    Ok(ServerHandle {
        port,
        shutdown,
        join: Some(join),
    })
}

/// The accept loop only accepts: each connection is served on its own thread,
/// so a capture mid-fetch (an OpenReview/doi.org call, a PDF download) neither
/// blocks the next capture nor wedges [`ServerHandle::stop`] — stopping joins
/// this loop (≤ one `ACCEPT_POLL` tick), while an in-flight request finishes on
/// its own thread after the listener is already released.
fn serve_loop(listener: TcpListener, cfg: Arc<ConnectorConfig>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let cfg = Arc::clone(&cfg);
                let spawned = std::thread::Builder::new()
                    .name("niutero-connector-req".to_string())
                    .spawn(move || {
                        // One bad connection must not kill the server. Debug,
                        // not warn: client-caused, and port scanners would
                        // spam warns.
                        if let Err(e) = handle_connection(&mut stream, &cfg) {
                            log::debug!("connector: connection error: {e}");
                        }
                    });
                if let Err(e) = spawned {
                    log::warn!("connector: could not spawn a request thread: {e}");
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(e) => {
                log::warn!("connector accept error: {e}");
                break;
            }
        }
    }
    log::info!("connector stopped");
}

fn handle_connection(stream: &mut TcpStream, cfg: &ConnectorConfig) -> io::Result<()> {
    let _ = stream.set_read_timeout(Some(SOCKET_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
    let response = match read_request(stream) {
        Ok(req) => {
            let (status, body) = route(&req, cfg);
            http_response(status, &body)
        }
        Err(ReadError::TooLarge) => {
            http_response("413 Payload Too Large", &json_error("request too large"))
        }
        Err(ReadError::Io(e)) => {
            log::debug!("connector: malformed request: {e}");
            http_response("400 Bad Request", &json_error("malformed request"))
        }
    };
    stream.write_all(response.as_bytes())
}

/// Dispatch a parsed request to a (status line, JSON body).
fn route(req: &Request, cfg: &ConnectorConfig) -> (&'static str, String) {
    // DNS-rebinding guard first: a non-loopback Host is refused outright.
    if !host_ok(req.host.as_deref()) {
        return ("403 Forbidden", json_error("non-loopback Host rejected"));
    }
    let (status, body) = match (req.method.as_str(), req.path.as_str()) {
        // Preflight politeness; we send no CORS headers anywhere.
        ("OPTIONS", _) => ("204 No Content", String::new()),
        ("GET", "/ping" | "/" | "/health") => {
            if !origin_ok(req.origin.as_deref()) {
                forbidden_origin()
            } else {
                let library = cfg.shared.lock().ok().and_then(|s| s.library.clone());
                ("200 OK", ping_body(library.as_deref()))
            }
        }
        ("POST", "/import") => {
            // Stricter than /ping: a mutating request needs a PRESENT
            // extension Origin — missing or `null` is refused.
            if !origin_ok_for_import(req.origin.as_deref()) {
                forbidden_origin()
            } else {
                handle_import(req, cfg)
            }
        }
        _ => ("404 Not Found", json_error("not found")),
    };
    // NEVER log req.body (user content).
    log::debug!("connector: {} {} -> {status}", req.method, req.path);
    (status, body)
}

fn handle_import(req: &Request, cfg: &ConnectorConfig) -> (&'static str, String) {
    let parsed: ImportRequest = match serde_json::from_str(&req.body) {
        Ok(r) => r,
        Err(e) => return ("400 Bad Request", json_error(&format!("bad JSON: {e}"))),
    };
    let root = match cfg.shared.lock().ok().and_then(|s| s.vault_root.clone()) {
        Some(r) => r,
        None => {
            return (
                "503 Service Unavailable",
                json_error("no library open in niutero"),
            )
        }
    };
    let mut v = match crate::open(&root) {
        Ok(v) => v,
        Err(e) => {
            return (
                "500 Internal Server Error",
                json_error(&format!("open library: {e}")),
            )
        }
    };
    // A resolve/import failure is reported as 200 {ok:false} so the extension
    // always reads a structured body (only transport/parse errors are non-200).
    match connector_import(&mut v, &parsed) {
        Ok(o) => {
            (cfg.on_import)(&o);
            ("200 OK", outcome_body(&o))
        }
        Err(e) => ("200 OK", json_error(&e)),
    }
}

// ------------------------------------------------------------ import pipeline

/// Resolve one capture and apply it to `v`, then run the import hooks
/// (enrich → normalize → PDF fetch → keep-updated refresh → auto-commit,
/// best-effort). The network resolve runs on the caller's thread (the server
/// thread), never a UI thread, and **without** the vault lock held.
pub fn connector_import(v: &mut Vault, req: &ImportRequest) -> Result<ImportOutcome, String> {
    // Honor the library's configured duplicate policy (Skip if unset), exactly
    // like every other import path.
    let policy = crate::default_dup_policy(v, crate::DupPolicy::Skip);

    // Resolve to entries (network happens here, no lock held), then re-key every
    // entry to the library's cite-key pattern so connector entries follow the
    // library convention and a re-capture of the same source renders the same
    // key (so the dup policy can dedupe it instead of letting a twin slip in).
    let resolved = resolve_entries(req)?;
    let fallback = resolved.fallback;
    let mut incoming = resolved.entries;
    for e in &mut incoming {
        rekey_to_base_pattern(v, e);
        e.validate()?;
    }

    // Under the vault lock (the read-modify-write of references.bib;
    // `merge_incoming`'s contract requires the caller to hold it — scoped so
    // the post-import hooks below can re-lock): content-identity triage, then
    // merge. A base-key collision is only a *duplicate* when it is the same
    // work; a DIFFERENT paper that happens to render the same key gets a
    // letter suffix and is added — never silently dropped. A same-work
    // collision under `rename` is suffixed here too, so connector adds use
    // the letter style (`key` → `keya`) rather than bulk-import's `key-2`.
    let report = {
        let _lock = crate::lock_vault(v)?;
        let items = crate::read_items(v)?;
        let existing: std::collections::HashMap<String, BibEntry> = entries(&items)
            .map(|e| (e.citekey.clone(), e.clone()))
            .collect();
        let mut taken: std::collections::HashSet<String> = existing.keys().cloned().collect();
        let mut pre_renamed: Vec<(String, String)> = Vec::new();
        for e in &mut incoming {
            if let Some(old) = existing.get(&e.citekey) {
                if !niutero_core::dedup::same_work(old, e) {
                    e.citekey = crate::next_free_key(&e.citekey, &taken).0;
                } else if policy == crate::DupPolicy::Rename {
                    let new = crate::next_free_key(&e.citekey, &taken).0;
                    pre_renamed.push((e.citekey.clone(), new.clone()));
                    e.citekey = new;
                }
                // Same work + Skip/Overwrite: leave the key; merge applies it.
            }
            taken.insert(e.citekey.clone());
        }
        let mut report = crate::merge_incoming(v, incoming, policy)?;
        // The pre-resolved renames reached merge as plain adds — fold them
        // back so counters (and the popup) tell the truth.
        for (old, new) in pre_renamed {
            report.added -= 1;
            report.added_keys.retain(|k| k != &new);
            report.renamed.push((old, new));
        }
        report
    };

    // Cover every entry this import wrote — added, renamed, AND overwritten — so
    // the connector's "clean entry" contract holds under every dup policy (under
    // `on_dup = overwrite`, a re-capture replaces the stored entry, which
    // `new_keys()` would otherwise omit).
    let touched = report.touched_keys();

    // Tags first (sidecar only) so any later hook sees a complete entry. A
    // skipped duplicate still gets the popup's tags — that capture was the
    // user's way of tagging the stored entry.
    let mut tag_targets = touched.clone();
    tag_targets.extend(report.skipped_keys.iter().cloned());
    let mut tags_updated = false;
    if !req.tags.is_empty() && !tag_targets.is_empty() {
        let adds: Vec<(String, Vec<String>)> = tag_targets
            .iter()
            .map(|k| (k.clone(), req.tags.clone()))
            .collect();
        match crate::set_tags_bulk(v, &adds) {
            Ok(_) => tags_updated = true,
            Err(e) => log::warn!("connector: tagging entries failed: {e}"),
        }
    }

    // The shared post-import pipeline (enrich → normalize → PDFs), with
    // normalization FORCED — the connector's whole job is to hand back a
    // clean, ready-to-use entry, independent of `normalize_on_import` (the
    // toggle that governs bulk/CLI imports, where you may want a verbatim copy).
    let hooks = crate::run_import_hooks(v, &touched, true);
    for w in &hooks.warnings {
        log::warn!("connector: {w}");
    }
    if hooks.normalized > 0 {
        log::info!("connector: normalized {} entr(ies)", hooks.normalized);
    }
    if hooks.pdfs.1 > 0 {
        log::info!(
            "connector: fetched {}/{} PDF(s)",
            hooks.pdfs.0,
            hooks.pdfs.1
        );
    }

    // Refresh keep-updated exports / auto-commit whenever the `.bib` changed —
    // an overwrite mutates it just as an add does.
    if report.added > 0 || report.overwritten > 0 || !report.renamed.is_empty() {
        for o in crate::refresh_exports(v).unwrap_or_default() {
            if let Some(e) = o.error {
                log::warn!(
                    "connector: keep-updated export to {} failed: {e}",
                    o.out.display()
                );
            }
        }
        match crate::auto_commit_if_enabled(v) {
            Ok(Some(msg)) => log::info!("connector: auto-committed: {msg}"),
            Ok(None) => {}
            Err(e) => log::warn!("connector: auto-commit failed: {e}"),
        }
    }

    // The entry to show in the popup: what was written, else the stored twin a
    // skip matched (so "Already in your library" can still name the paper).
    let shown = touched
        .first()
        .or_else(|| report.skipped_keys.first())
        .cloned();
    let (citekey, title) = match shown {
        Some(k) => {
            let title = crate::show(v, &k)
                .ok()
                .and_then(|view| view.fields.get("title").cloned())
                .unwrap_or_default();
            (k, display_title(&title))
        }
        None => (String::new(), String::new()),
    };
    Ok(ImportOutcome {
        citekey,
        title,
        added: report.added,
        overwritten: report.overwritten,
        skipped: report.skipped,
        renamed: report.renamed.len(),
        tags_updated,
        fallback,
    })
}

/// The title as the popup should show it: brace protection stripped. The
/// stored entry keeps its `{{…}}` — this is display-only.
fn display_title(s: &str) -> String {
    s.replace(['{', '}'], "")
}

/// What a capture resolved to: the entries to merge, and — when the
/// identifier's canonical resolution failed but the page's scraped metadata
/// was usable — the reason the capture fell back to it.
#[derive(Debug)]
struct Resolved {
    entries: Vec<BibEntry>,
    fallback: Option<String>,
}

/// Resolve one capture to the BibTeX entries to merge, preferring a canonical
/// source over scraped metadata. Network fetches (OpenReview / doi.org) run
/// here, on the caller's thread, **without** the vault lock held.
///
/// A resolution failure no longer loses the capture: when the request also
/// carries usable page metadata, the entry is built from that and the outcome
/// marked as a fallback (a transient doi.org 5xx or a venue without BibTeX
/// must not throw away a paper the extension already described).
fn resolve_entries(req: &ImportRequest) -> Result<Resolved, String> {
    // Fall back to the scraped metadata for `reason`, or surface `reason` as
    // the error when the metadata can't make an entry either.
    let identifier = req
        .identifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let fall_back = |reason: String| -> Result<Resolved, String> {
        match req.metadata.as_ref().map(build_entry_from_metadata) {
            Some(Ok(mut e)) => {
                // The identifier that failed to *resolve* is still the paper's
                // identity: keep it on the entry so `enrich` (a DOI) or the
                // arXiv pass (an eprint) can finish the job later.
                if let Some(id) = identifier {
                    attach_identifier(&mut e, id);
                }
                log::info!("connector: using page metadata ({reason})");
                Ok(Resolved {
                    entries: vec![e],
                    fallback: Some(reason),
                })
            }
            _ => Err(reason),
        }
    };
    let ok = |entries: Vec<BibEntry>| Resolved {
        entries,
        fallback: None,
    };
    if let Some(id) = req
        .identifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        // An OpenReview capture is OpenReview-ONLY: never fall through to doi.org.
        // (Doing so turns an unreadable id into a baffling `https://doi.org/
        // openreview:…` → HTTP 400 instead of a clear OpenReview error.) The
        // forum page exposes no usable DOI, but the venue's canonical BibTeX is
        // one API call away — far better than the page's sparse meta tags. On
        // failure the fallback is the page METADATA, never doi.org.
        if is_openreview_identifier(id) {
            let Some(or_id) = openreview_id(id) else {
                return fall_back(format!(
                    "'{id}' is an OpenReview link but no submission id could be read from it"
                ));
            };
            return match niutero_online::fetch_openreview_bibtex(&or_id)
                .and_then(|src| parsed_entries(&src, &format!("OpenReview {or_id}")))
            {
                Ok(es) => Ok(ok(es)),
                Err(e) => fall_back(e),
            };
        }
        // Otherwise a DOI / arXiv id, resolved via doi.org content negotiation.
        return match niutero_online::fetch_doi_bibtex(&identifier_to_doi(id))
            .and_then(|src| parsed_entries(&src, id))
        {
            Ok(es) => Ok(ok(es)),
            Err(e) => fall_back(e),
        };
    }
    if let Some(meta) = &req.metadata {
        return Ok(ok(vec![build_entry_from_metadata(meta)?]));
    }
    Err("the capture had neither an identifier nor metadata".into())
}

/// Put a capture's identifier onto a metadata-built entry: a DOI as `doi`
/// (unless one is present), an arXiv id as `eprint` + `archiveprefix`. An
/// OpenReview id has no BibTeX field — the page url already names it.
fn attach_identifier(e: &mut BibEntry, id: &str) {
    let id = id.trim();
    if is_openreview_identifier(id) {
        return;
    }
    if let Some(rest) = id
        .strip_prefix("arXiv:")
        .or_else(|| id.strip_prefix("arxiv:"))
        .or_else(|| id.strip_prefix("arXiv/"))
    {
        let rest = rest.trim();
        if !rest.is_empty() {
            e.set("eprint", rest);
            e.set("archiveprefix", "arXiv");
        }
        return;
    }
    let doi = id.strip_prefix("doi:").unwrap_or(id).trim();
    if !doi.is_empty() && e.get("doi").is_none_or(|d| d.trim().is_empty()) {
        e.set("doi", doi);
    }
}

/// Parse fetched BibTeX into entries, erroring if it held none.
fn parsed_entries(src: &str, source: &str) -> Result<Vec<BibEntry>, String> {
    let es: Vec<BibEntry> = entries(&parse(src)).cloned().collect();
    if es.is_empty() {
        return Err(format!("no BibTeX entries resolved from {source}"));
    }
    Ok(es)
}

/// Is this identifier meant for OpenReview — an `openreview:<id>` (what the
/// extension sends) or any `openreview.net` URL? Such an identifier is routed to
/// the OpenReview resolver *only*, never to doi.org.
fn is_openreview_identifier(identifier: &str) -> bool {
    let lower = identifier.trim().to_ascii_lowercase();
    lower.starts_with("openreview:") || lower.contains("openreview.net")
}

/// Extract an OpenReview submission id from a connector identifier: either an
/// explicit `openreview:<id>` (what the extension sends) or any `openreview.net`
/// URL carrying an `id=<id>` query parameter. Returns `None` when the id isn't a
/// plain token — so it can never inject into the API URL the online layer builds
/// (it's interpolated unescaped), and the caller surfaces a clear error.
fn openreview_id(identifier: &str) -> Option<String> {
    let s = identifier.trim();
    let lower = s.to_ascii_lowercase();
    let raw: String = if lower.starts_with("openreview:") {
        // The prefix is 11 ASCII bytes regardless of case; slice the original to
        // keep the (case-sensitive) id intact.
        s["openreview:".len()..].trim().to_string()
    } else if lower.contains("openreview.net") {
        // Drop any `#fragment` (e.g. `forum?id=X#discussion`) before reading the
        // query, so the anchor doesn't get glued onto the id.
        let query = s
            .split_once('?')
            .map(|(_, q)| q)
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix("id="))?
            .to_string()
    } else {
        return None;
    };
    let id = raw.trim();
    (!id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    .then(|| id.to_string())
}

/// Set `e.citekey` to the library's BASE cite-key pattern (no uniquifying
/// suffix), falling back to `"ref"` if the pattern renders empty. The base key
/// is intentional: re-capturing the same source renders the same key, so the dup
/// policy can skip/rename it rather than let a twin slip in. Computed from the
/// entry's pre-normalization fields, so a re-capture matches the stored key.
fn rekey_to_base_pattern(v: &Vault, e: &mut BibEntry) {
    let base = crate::resolve_pattern(v, None).render(e);
    e.citekey = if base.trim().is_empty() {
        "ref".to_string()
    } else {
        base
    };
}

/// Map a capture identifier to a DOI for the engine's doi.org resolver. An
/// `arXiv:<id>` becomes the versionless DataCite DOI arXiv mints; a `doi:` prefix
/// is stripped; anything else is treated as a bare DOI.
fn identifier_to_doi(id: &str) -> String {
    let id = id.trim();
    if let Some(rest) = id
        .strip_prefix("arXiv:")
        .or_else(|| id.strip_prefix("arxiv:"))
        .or_else(|| id.strip_prefix("arXiv/"))
    {
        return format!("10.48550/arXiv.{}", strip_arxiv_version(rest.trim()));
    }
    id.strip_prefix("doi:").unwrap_or(id).trim().to_string()
}

/// Drop a trailing `v<N>` version suffix from an arXiv id (the base DataCite DOI
/// is versionless).
fn strip_arxiv_version(id: &str) -> &str {
    if let Some(pos) = id.rfind('v') {
        if id[pos + 1..].chars().all(|c| c.is_ascii_digit()) && pos + 1 < id.len() {
            return &id[..pos];
        }
    }
    id
}

/// Build a [`BibEntry`] from scraped metadata — the offline fallback when a
/// capture carries no resolvable identifier. The caller re-keys it to the
/// library pattern and validates (so this leaves `citekey` empty).
fn build_entry_from_metadata(m: &ScrapedMetadata) -> Result<BibEntry, String> {
    let title = m.title.trim();
    if title.is_empty() {
        return Err("the page had no title to build an entry from".into());
    }
    let entry_type = classify(m);
    let mut e = BibEntry::new(entry_type, "");
    e.set("title", title);

    let authors: Vec<String> = m
        .authors
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if !authors.is_empty() {
        e.set("author", authors.join(" and "));
    }
    set_if("year", &m.year, &mut e);
    match entry_type {
        "article" if !m.journal.trim().is_empty() => e.set("journal", m.journal.trim()),
        "inproceedings" | "incollection" if !m.booktitle.trim().is_empty() => {
            e.set("booktitle", m.booktitle.trim())
        }
        // A venue-typed page whose venue tag was blank: an `@inproceedings`
        // with no booktitle is a hollow shell — a `@misc` is honest, and the
        // journal (if any) is still worth keeping.
        "inproceedings" | "incollection" | "article" => {
            e.set_type("misc");
            set_if("journal", &m.journal, &mut e);
        }
        _ => {}
    }
    set_if("volume", &m.volume, &mut e);
    set_if("number", &m.issue, &mut e);
    set_if("pages", &m.pages, &mut e);
    set_if("publisher", &m.publisher, &mut e);
    set_if("doi", &m.doi, &mut e);
    set_if("url", &m.url, &mut e);
    Ok(e)
}

fn set_if(field: &str, value: &str, e: &mut BibEntry) {
    let v = value.trim();
    if !v.is_empty() {
        e.set(field, v);
    }
}

fn classify(m: &ScrapedMetadata) -> &'static str {
    match m.item_type.trim().to_ascii_lowercase().as_str() {
        "conference" | "proceedings" | "inproceedings" => "inproceedings",
        "article" | "journal" => "article",
        "book" => "book",
        "chapter" | "incollection" => "incollection",
        "thesis" | "dissertation" => "phdthesis",
        "report" | "techreport" => "techreport",
        _ if !m.journal.trim().is_empty() => "article",
        _ if !m.booktitle.trim().is_empty() => "inproceedings",
        _ => "misc",
    }
}

// ------------------------------------------------------------- security layer

/// A loopback `Host` (defeats DNS rebinding). A missing `Host` is allowed — the
/// `127.0.0.1` bind is the real barrier.
fn host_ok(host: Option<&str>) -> bool {
    host.is_none_or(host_is_loopback)
}

/// Pure: does this `Host` value name a loopback address? Strips an optional
/// `:port` and the brackets of an IPv6 literal first.
fn host_is_loopback(value: &str) -> bool {
    let v = value.trim();
    let host = if let Some(rest) = v.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest) // [::1] / [::1]:port
    } else {
        match v.rsplit_once(':') {
            Some((h, _)) => h,
            None => v,
        }
    };
    host.eq_ignore_ascii_case("127.0.0.1")
        || host.eq_ignore_ascii_case("localhost")
        || host == "::1"
}

/// Origin rule for the read-only routes (`/ping`): an extension `Origin`, or
/// none at all (`curl` debugging; the loopback bind already gates us, and the
/// route leaks nothing).
fn origin_ok(origin: Option<&str>) -> bool {
    origin.is_none_or(origin_is_extension)
}

/// Origin rule for the mutating route (`POST /import`): a PRESENT extension
/// `Origin` is required. The extension's cross-origin fetches always send one;
/// a missing `Origin` or the opaque `Origin: null` (what a sandboxed iframe or
/// a `file:` page sends) would be a blind-POST CSRF hole and is refused.
fn origin_ok_for_import(origin: Option<&str>) -> bool {
    origin.is_some_and(origin_is_extension)
}

/// Pure: is this `Origin` an extension scheme? `null` is NOT accepted — any
/// hostile page can forge it; no extension needs it.
fn origin_is_extension(value: &str) -> bool {
    let o = value.trim();
    o.starts_with("chrome-extension://")
        || o.starts_with("moz-extension://")
        || o.starts_with("safari-web-extension://")
}

fn forbidden_origin() -> (&'static str, String) {
    ("403 Forbidden", json_error("origin not allowed"))
}

// ------------------------------------------------------------- HTTP plumbing

fn http_response(status: &str, body: &str) -> String {
    // Deliberately no Access-Control-Allow-* headers: the extension uses host
    // permissions (CORS-exempt); a web page's script gets opaque failures.
    format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

fn ping_body(library: Option<&str>) -> String {
    let lib = library.map(json_str).unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"app\":\"niutero\",\"ok\":true,\"version\":{},\"library\":{}}}",
        json_str(env!("CARGO_PKG_VERSION")),
        lib
    )
}

fn outcome_body(o: &ImportOutcome) -> String {
    let mut s = format!(
        "{{\"ok\":true,\"citekey\":{},\"title\":{},\"added\":{},\"overwritten\":{},\
         \"skipped\":{},\"renamed\":{},\"tags_updated\":{}",
        json_str(&o.citekey),
        json_str(&o.title),
        o.added,
        o.overwritten,
        o.skipped,
        o.renamed,
        o.tags_updated
    );
    if let Some(f) = &o.fallback {
        s.push_str(",\"fallback\":");
        s.push_str(&json_str(f));
    }
    s.push('}');
    s
}

fn json_error(msg: &str) -> String {
    format!("{{\"ok\":false,\"error\":{}}}", json_str(msg))
}

/// A minimal JSON string literal (quoted, with the mandatory escapes).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A parsed HTTP request: just what the connector needs.
struct Request {
    method: String,
    path: String,
    body: String,
    host: Option<String>,
    origin: Option<String>,
}

enum ReadError {
    /// Declared `Content-Length` over [`MAX_BODY`] — refused before any
    /// allocation, so a hostile length can't OOM the process.
    TooLarge,
    Io(io::Error),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        ReadError::Io(e)
    }
}

/// Read one HTTP/1.1 request: the request line + headers, then exactly the
/// declared number of body bytes.
fn read_request(stream: &TcpStream) -> Result<Request, ReadError> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        // Cap the head like the body — a drip-fed header stream must not grow
        // memory or wedge the single-threaded accept loop.
        if head.len() + line.len() > MAX_HEAD {
            return Err(ReadError::TooLarge);
        }
        head.push_str(&line);
    }
    let (method, path) = request_line(&head);
    let declared = content_length(&head);
    if declared > MAX_BODY {
        return Err(ReadError::TooLarge);
    }
    let mut body = vec![0u8; declared];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
        host: header(&head, "host"),
        origin: header(&head, "origin"),
    })
}

/// Method and path (sans query) from the first line ("POST /import HTTP/1.1").
fn request_line(head: &str) -> (String, String) {
    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts
        .next()
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    (method, path)
}

/// A header value by (case-insensitive) name.
fn header(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

/// The `Content-Length` header value (0 if absent/invalid).
fn content_length(head: &str) -> usize {
    header(head, "content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    struct RegistryEnv {
        _dir: tempfile::TempDir,
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    fn isolated_registry() -> RegistryEnv {
        let guard = crate::test_registry_env::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NIUTERO_REGISTRY", dir.path().join("vaults.toml"));
        RegistryEnv {
            _dir: dir,
            _guard: guard,
        }
    }

    /// Start a real server against `shared`, send one raw request, return the
    /// raw response.
    fn run_request(shared: Arc<Mutex<ConnectorShared>>, request: &str) -> String {
        let cfg = ConnectorConfig {
            port: 0,
            shared,
            on_import: Arc::new(|_| {}),
        };
        let handle = start(cfg).unwrap();
        let port = handle.port();
        let req = request.to_string();
        let join = thread::spawn(move || {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            client.write_all(req.as_bytes()).unwrap();
            let mut response = String::new();
            let _ = client.read_to_string(&mut response);
            response
        });
        let response = join.join().unwrap();
        handle.stop();
        response
    }

    fn shared_for(root: Option<PathBuf>, library: Option<&str>) -> Arc<Mutex<ConnectorShared>> {
        Arc::new(Mutex::new(ConnectorShared {
            vault_root: root,
            library: library.map(str::to_string),
        }))
    }

    #[test]
    fn host_loopback_accept_and_reject() {
        for h in [
            "127.0.0.1",
            "127.0.0.1:23510",
            "localhost",
            "localhost:23510",
            "LocalHost:8080",
            "[::1]",
            "[::1]:23510",
        ] {
            assert!(host_is_loopback(h), "should accept Host {h:?}");
        }
        for h in [
            "evil.example.com",
            "evil.example.com:23510",
            "192.168.1.10",
            "10.0.0.5:23510",
            "0.0.0.0",
            "127.0.0.1.evil.com",
        ] {
            assert!(!host_is_loopback(h), "should reject Host {h:?}");
        }
        // Missing Host is allowed (the bind is the barrier). Missing Origin is
        // allowed on the read-only routes — but never on /import.
        assert!(host_ok(None));
        assert!(origin_ok(None));
        assert!(!origin_ok_for_import(None));
    }

    #[test]
    fn origin_extension_accept_and_reject() {
        for o in [
            "chrome-extension://abcdefghijklmnop",
            "moz-extension://1234-5678",
            "safari-web-extension://deadbeef",
        ] {
            assert!(origin_is_extension(o), "should accept Origin {o:?}");
        }
        for o in [
            "https://evil.example.com",
            "http://localhost:3000",
            "https://niutero.example",
            // `null` is what a sandboxed iframe / file: page sends — a blind
            // CSRF write if accepted. Refused.
            "null",
            "",
        ] {
            assert!(!origin_is_extension(o), "should reject Origin {o:?}");
        }
    }

    #[test]
    fn arxiv_identifier_maps_to_versionless_datacite_doi() {
        assert_eq!(
            identifier_to_doi("arXiv:2301.00001"),
            "10.48550/arXiv.2301.00001"
        );
        assert_eq!(
            identifier_to_doi("arXiv:2301.00001v3"),
            "10.48550/arXiv.2301.00001"
        );
        assert_eq!(identifier_to_doi("doi:10.1/x"), "10.1/x");
        assert_eq!(identifier_to_doi("10.1/x"), "10.1/x");
    }

    #[test]
    fn openreview_id_parses_prefix_and_url_forms() {
        assert_eq!(
            openreview_id("openreview:2DtxPCL3T5").as_deref(),
            Some("2DtxPCL3T5")
        );
        // Case-insensitive scheme; the id's own case is preserved.
        assert_eq!(
            openreview_id("OpenReview:Abc_1-2.3").as_deref(),
            Some("Abc_1-2.3")
        );
        assert_eq!(
            openreview_id("https://openreview.net/forum?id=2DtxPCL3T5").as_deref(),
            Some("2DtxPCL3T5")
        );
        // The forum `id` wins over a sibling `noteId`.
        assert_eq!(
            openreview_id("https://openreview.net/pdf?id=XyZ9&noteId=qq").as_deref(),
            Some("XyZ9")
        );
        // A trailing `#fragment` (a real OpenReview anchor) is dropped, not glued
        // onto the id.
        assert_eq!(
            openreview_id("https://openreview.net/forum?id=2DtxPCL3T5#discussion").as_deref(),
            Some("2DtxPCL3T5")
        );
        // Non-OpenReview identifiers fall through to the DOI/arXiv path.
        assert_eq!(openreview_id("10.1145/3292500"), None);
        assert_eq!(openreview_id("arXiv:2301.00001"), None);
        // An id with URL-injecting characters (or empty) is refused.
        assert_eq!(openreview_id("openreview:a&b=c"), None);
        assert_eq!(openreview_id("openreview:"), None);
        assert_eq!(openreview_id("https://openreview.net/forum"), None);
        // A venue/group id (slashes) is not a submission id → refused.
        assert_eq!(openreview_id("openreview:ICLR.cc/2024/Conference"), None);
    }

    #[test]
    fn an_openreview_identifier_never_falls_through_to_doi() {
        // The bug behind a real "doi.org HTTP 400": an OpenReview link whose id
        // can't be read must yield a clear OpenReview error, NOT get shipped to
        // doi.org as `https://doi.org/openreview:…`. This errors before any
        // network call (the id is unreadable), so no connectivity is needed.
        assert!(is_openreview_identifier(
            "openreview:ICLR.cc/2024/Conference"
        ));
        let req = ImportRequest {
            identifier: Some("openreview:ICLR.cc/2024/Conference".into()),
            metadata: None,
            tags: vec![],
        };
        let err = resolve_entries(&req).unwrap_err();
        assert!(
            err.to_lowercase().contains("openreview"),
            "expected an OpenReview error, got: {err}"
        );
        assert!(
            !err.contains("doi.org"),
            "must not attempt doi.org for an OpenReview link, got: {err}"
        );
    }

    #[test]
    fn connector_always_normalizes_even_with_the_toggle_off() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        // A fresh vault: `normalize_on_import` is off by default.
        let mut v = crate::init(dir.path()).unwrap();
        assert!(!v.config.workflow.normalize_on_import);
        let req = ImportRequest {
            identifier: None,
            metadata: Some(ScrapedMetadata {
                title: "Captured Paper Title".into(),
                authors: vec!["Doe, Jane".into()],
                year: "2024".into(),
                journal: "Some Journal".into(),
                ..Default::default()
            }),
            tags: vec![],
        };
        let out = connector_import(&mut v, &req).unwrap();
        assert_eq!(out.added, 1);

        let reopened = crate::open(dir.path()).unwrap();
        let listed = crate::list(&reopened, crate::Filter::All).unwrap();
        let key = listed[0].citekey.clone();
        let title = crate::show(&reopened, &key)
            .unwrap()
            .fields
            .get("title")
            .cloned()
            .unwrap_or_default();
        // `protect_title_caps` (a default rule) wrapped the capitalized words in
        // `{{…}}` — proof the connector normalized despite the toggle being off.
        assert!(
            title.contains("{{"),
            "expected a normalized title, got: {title:?}"
        );
    }

    #[test]
    fn connector_overwrite_recapture_still_normalizes_and_retags() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let mut v = crate::init(dir.path()).unwrap();
        // Opt into overwrite-on-duplicate (the policy the review flagged): a
        // re-capture replaces the entry, which `new_keys()` omits — so the hooks
        // must use `touched_keys()` to keep the "always clean" contract.
        crate::set_workflow(&mut v, None, None, Some("overwrite"), None, None).unwrap();
        let make = || ScrapedMetadata {
            title: "Captured Paper Title".into(),
            authors: vec!["Doe, Jane".into()],
            year: "2024".into(),
            journal: "Some Journal".into(),
            ..Default::default()
        };

        let first = connector_import(
            &mut v,
            &ImportRequest {
                identifier: None,
                metadata: Some(make()),
                tags: vec![],
            },
        )
        .unwrap();
        assert_eq!((first.added, first.overwritten), (1, 0));

        // Re-capture the same page (a fresh vault, as the server opens per
        // request): it collides and OVERWRITES.
        let mut v2 = crate::open(dir.path()).unwrap();
        let second = connector_import(
            &mut v2,
            &ImportRequest {
                identifier: None,
                metadata: Some(make()),
                tags: vec!["updated".into()],
            },
        )
        .unwrap();
        assert_eq!(
            (second.added, second.overwritten),
            (0, 1),
            "a re-capture under overwrite must replace, not add"
        );

        let reopened = crate::open(dir.path()).unwrap();
        let listed = crate::list(&reopened, crate::Filter::All).unwrap();
        assert_eq!(listed.len(), 1, "overwrite must not create a twin");
        let key = listed[0].citekey.clone();
        let title = crate::show(&reopened, &key)
            .unwrap()
            .fields
            .get("title")
            .cloned()
            .unwrap_or_default();
        // The overwritten entry is still normalized, and the re-capture's tag was
        // applied — both would be skipped if the hooks used `new_keys()`.
        assert!(
            title.contains("{{"),
            "overwritten entry must be normalized, got: {title:?}"
        );
        assert!(
            crate::current_tags(&reopened, &key)
                .unwrap()
                .contains(&"updated".to_string()),
            "tags must be applied on an overwrite re-capture"
        );
    }

    #[test]
    fn ping_reports_the_open_library() {
        let _env = isolated_registry();
        let shared = shared_for(None, Some("MyLib"));
        let resp = run_request(
            shared,
            "GET /ping HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: chrome-extension://abc\r\n\r\n",
        );
        assert!(resp.starts_with("HTTP/1.1 200 OK"), "got: {resp}");
        assert!(resp.contains("\"library\":\"MyLib\""), "got: {resp}");
        assert!(!resp.contains("Access-Control-Allow"), "got: {resp}");
    }

    #[test]
    fn import_from_metadata_adds_an_entry_offline() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let shared = shared_for(Some(v.root.clone()), Some("L"));
        let body = r#"{"metadata":{"title":"Captured Paper","authors":["Doe, Jane"],"year":"2024","journal":"J"},"tags":["web"]}"#;
        let resp = run_request(
            shared,
            &format!(
                "POST /import HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: chrome-extension://abc\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(resp.starts_with("HTTP/1.1 200 OK"), "got: {resp}");
        assert!(resp.contains("\"ok\":true"), "got: {resp}");
        assert!(resp.contains("\"added\":1"), "got: {resp}");
        // The response title is for the popup: brace protection stripped
        // (the STORED entry keeps its {{…}} — other tests pin that).
        assert!(
            !resp.contains("{{"),
            "response title must be display-clean, got: {resp}"
        );

        let reopened = crate::open(dir.path()).unwrap();
        // Keyed by the library pattern, tagged in the sidecar.
        let listed = crate::list(&reopened, crate::Filter::All).unwrap();
        assert_eq!(listed.len(), 1);
        let key = &listed[0].citekey;
        assert!(crate::current_tags(&reopened, key)
            .unwrap()
            .contains(&"web".to_string()));
    }

    #[test]
    fn reimporting_the_same_metadata_skips_under_the_default_policy() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let mut v = crate::init(dir.path()).unwrap();
        let req = ImportRequest {
            identifier: None,
            metadata: Some(ScrapedMetadata {
                title: "Attention Is All You Need".into(),
                authors: vec!["Vaswani, Ashish".into()],
                year: "2017".into(),
                journal: "NeurIPS".into(),
                ..Default::default()
            }),
            tags: vec![],
        };
        let first = connector_import(&mut v, &req).unwrap();
        assert_eq!((first.added, first.skipped), (1, 0));
        // The server opens a fresh vault per request; re-capturing the same page
        // must render the same base key and be skipped, not added as a twin.
        let mut v2 = crate::open(dir.path()).unwrap();
        let second = connector_import(&mut v2, &req).unwrap();
        assert_eq!(
            (second.added, second.skipped),
            (0, 1),
            "a re-capture of the same page must dedupe"
        );
    }

    #[test]
    fn import_rejects_a_web_origin() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let shared = shared_for(Some(v.root.clone()), Some("L"));
        let body = r#"{"metadata":{"title":"X"}}"#;
        let resp = run_request(
            shared,
            &format!(
                "POST /import HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(resp.starts_with("HTTP/1.1 403"), "got: {resp}");
        // The web-origin request was refused before any import.
        let reopened = crate::open(dir.path()).unwrap();
        assert!(crate::list(&reopened, crate::Filter::All)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn import_with_no_library_open_is_503() {
        let _env = isolated_registry();
        let shared = shared_for(None, None);
        let body = r#"{"metadata":{"title":"X"}}"#;
        let resp = run_request(
            shared,
            &format!(
                "POST /import HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: chrome-extension://abc\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(resp.starts_with("HTTP/1.1 503"), "got: {resp}");
    }

    #[test]
    fn non_loopback_host_is_403() {
        let _env = isolated_registry();
        let shared = shared_for(None, Some("L"));
        let resp = run_request(
            shared,
            "GET /ping HTTP/1.1\r\nHost: evil.example.com\r\nOrigin: chrome-extension://abc\r\n\r\n",
        );
        assert!(resp.starts_with("HTTP/1.1 403"), "got: {resp}");
    }

    #[test]
    fn import_rejects_null_and_missing_origin() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let body = r#"{"metadata":{"title":"X"}}"#;
        for origin_header in ["Origin: null\r\n", ""] {
            let resp = run_request(
                shared_for(Some(v.root.clone()), Some("L")),
                &format!(
                    "POST /import HTTP/1.1\r\nHost: 127.0.0.1\r\n{origin_header}\
                     Content-Length: {}\r\n\r\n{body}",
                    body.len()
                ),
            );
            assert!(
                resp.starts_with("HTTP/1.1 403"),
                "origin {origin_header:?} must be refused, got: {resp}"
            );
        }
        // ...while /ping without an Origin still answers (curl debugging).
        let resp = run_request(
            shared_for(None, Some("L")),
            "GET /ping HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        );
        assert!(resp.starts_with("HTTP/1.1 200"), "got: {resp}");
        // and no import happened
        let reopened = crate::open(dir.path()).unwrap();
        assert!(crate::list(&reopened, crate::Filter::All)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn oversized_headers_are_413() {
        let _env = isolated_registry();
        let junk = "x".repeat(MAX_HEAD + 1024);
        let resp = run_request(
            shared_for(None, Some("L")),
            &format!("GET /ping HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Junk: {junk}\r\n\r\n"),
        );
        assert!(resp.starts_with("HTTP/1.1 413"), "got: {resp}");
    }

    #[test]
    fn different_paper_with_colliding_key_is_added_with_letter_suffix() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let mut v = crate::init(dir.path()).unwrap();
        let capture = |title: &str| ImportRequest {
            identifier: None,
            metadata: Some(ScrapedMetadata {
                title: title.into(),
                authors: vec!["Mu, Jesse".into()],
                year: "2023".into(),
                journal: "J".into(),
                ..Default::default()
            }),
            tags: vec![],
        };
        let first = connector_import(&mut v, &capture("Learning to Compress Prompts")).unwrap();
        assert_eq!(first.added, 1);

        // A DIFFERENT paper whose base key collides must be ADDED under a
        // suffixed key — never silently "skipped as a duplicate".
        let mut v2 = crate::open(dir.path()).unwrap();
        let second = connector_import(&mut v2, &capture("Learning to Compress Videos")).unwrap();
        assert_eq!(
            (second.added, second.skipped),
            (1, 0),
            "a different paper must not be dropped as a duplicate"
        );
        let reopened = crate::open(dir.path()).unwrap();
        let listed = crate::list(&reopened, crate::Filter::All).unwrap();
        assert_eq!(listed.len(), 2, "both papers must be in the library");
        assert!(
            second.citekey.starts_with(&first.citekey) && second.citekey != first.citekey,
            "expected a suffixed key, got {:?} vs {:?}",
            second.citekey,
            first.citekey
        );
    }

    #[test]
    fn rename_policy_reports_renamed_with_letter_suffix() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let mut v = crate::init(dir.path()).unwrap();
        crate::set_workflow(&mut v, None, None, Some("rename"), None, None).unwrap();
        let req = ImportRequest {
            identifier: None,
            metadata: Some(ScrapedMetadata {
                title: "Captured Paper Title".into(),
                authors: vec!["Doe, Jane".into()],
                year: "2024".into(),
                journal: "J".into(),
                ..Default::default()
            }),
            tags: vec![],
        };
        let first = connector_import(&mut v, &req).unwrap();
        assert_eq!((first.added, first.renamed), (1, 0));

        // Re-capturing the SAME paper under `rename` adds a suffixed twin —
        // and must say so ("renamed"), never "already in your library".
        let mut v2 = crate::open(dir.path()).unwrap();
        let second = connector_import(&mut v2, &req).unwrap();
        assert_eq!(
            (second.added, second.renamed, second.skipped),
            (0, 1, 0),
            "a rename must be reported as renamed"
        );
        assert!(
            second.citekey.starts_with(&first.citekey) && second.citekey != first.citekey,
            "letter-suffix style expected: {:?} vs {:?}",
            second.citekey,
            first.citekey
        );
        let body = outcome_body(&second);
        assert!(body.contains("\"renamed\":1"), "got: {body}");
    }

    #[test]
    fn skip_recapture_still_applies_tags() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let mut v = crate::init(dir.path()).unwrap();
        let make = |tags: Vec<String>| ImportRequest {
            identifier: None,
            metadata: Some(ScrapedMetadata {
                title: "Captured Paper Title".into(),
                authors: vec!["Doe, Jane".into()],
                year: "2024".into(),
                journal: "J".into(),
                ..Default::default()
            }),
            tags,
        };
        let first = connector_import(&mut v, &make(vec![])).unwrap();
        assert_eq!(first.added, 1);

        // Re-capture with a tag: skipped as a duplicate, but the tag must land
        // on the stored entry (that capture WAS the user's tagging gesture).
        let mut v2 = crate::open(dir.path()).unwrap();
        let second = connector_import(&mut v2, &make(vec!["to-read".into()])).unwrap();
        assert_eq!((second.added, second.skipped), (0, 1));
        assert!(second.tags_updated, "tags_updated must be reported");
        assert_eq!(second.citekey, first.citekey, "the stored twin is named");
        let body = outcome_body(&second);
        assert!(body.contains("\"tags_updated\":true"), "got: {body}");

        let reopened = crate::open(dir.path()).unwrap();
        assert!(
            crate::current_tags(&reopened, &first.citekey)
                .unwrap()
                .contains(&"to-read".to_string()),
            "the popup tag must reach the stored entry"
        );
    }

    #[test]
    fn resolve_failure_with_usable_metadata_falls_back() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let mut v = crate::init(dir.path()).unwrap();
        // An OpenReview link whose id can't be read (a profile/group page):
        // with usable page metadata the capture must still land, marked as a
        // fallback — not error out.
        let req = ImportRequest {
            identifier: Some("openreview:ICLR.cc/2024/Conference".into()),
            metadata: Some(ScrapedMetadata {
                title: "Rescued From Metadata".into(),
                authors: vec!["Doe, Jane".into()],
                year: "2024".into(),
                booktitle: "Some Venue".into(),
                item_type: "conference".into(),
                ..Default::default()
            }),
            tags: vec![],
        };
        let out = connector_import(&mut v, &req).unwrap();
        assert_eq!(out.added, 1);
        let reason = out
            .fallback
            .clone()
            .expect("outcome must be marked as a fallback");
        assert!(
            reason.to_lowercase().contains("openreview"),
            "got: {reason}"
        );
        let body = outcome_body(&out);
        assert!(body.contains("\"fallback\":"), "got: {body}");
    }

    #[test]
    fn resolve_failure_without_metadata_still_errors() {
        let req = ImportRequest {
            identifier: Some("openreview:ICLR.cc/2024/Conference".into()),
            metadata: None,
            tags: vec![],
        };
        let err = resolve_entries(&req).unwrap_err();
        assert!(err.to_lowercase().contains("openreview"), "got: {err}");
    }

    #[test]
    fn metadata_fallback_keeps_the_failed_identifier_on_the_entry() {
        let mut e = BibEntry::new("misc", "").with_field("title", "T");
        attach_identifier(&mut e, "10.1145/1234.5678");
        assert_eq!(e.get("doi"), Some("10.1145/1234.5678"));
        // an existing doi is never clobbered
        attach_identifier(&mut e, "doi:10.9/other");
        assert_eq!(e.get("doi"), Some("10.1145/1234.5678"));
        let mut a = BibEntry::new("misc", "").with_field("title", "T");
        attach_identifier(&mut a, "arXiv:2301.00001v2");
        assert_eq!(a.get("eprint"), Some("2301.00001v2"));
        assert_eq!(a.get("archiveprefix"), Some("arXiv"));
        let mut o = BibEntry::new("misc", "").with_field("title", "T");
        attach_identifier(&mut o, "openreview:abc");
        assert_eq!(o.fields.len(), 1, "an OpenReview id has no BibTeX field");
    }

    #[test]
    fn venue_typed_page_without_a_venue_becomes_misc() {
        // `citation_conference_title` present but blank: an @inproceedings
        // with no booktitle is a hollow shell.
        let m = ScrapedMetadata {
            title: "Hollow".into(),
            item_type: "conference".into(),
            booktitle: "   ".into(),
            ..Default::default()
        };
        let e = build_entry_from_metadata(&m).unwrap();
        assert_eq!(e.entry_type(), "misc");
        assert_eq!(e.get("booktitle"), None);
        // ...while a real venue keeps its type
        let m = ScrapedMetadata {
            title: "Solid".into(),
            item_type: "conference".into(),
            booktitle: "Some Conference".into(),
            ..Default::default()
        };
        assert_eq!(
            build_entry_from_metadata(&m).unwrap().entry_type(),
            "inproceedings"
        );
    }

    #[test]
    fn requests_are_served_concurrently() {
        // Two pings on two connections at once: with per-connection threads
        // the second must not wait for the first (the old single-threaded
        // accept loop serialized every capture behind an in-flight fetch).
        let _env = isolated_registry();
        let shared = shared_for(None, Some("L"));
        let cfg = ConnectorConfig {
            port: 0,
            shared,
            on_import: Arc::new(|_| {}),
        };
        let handle = start(cfg).unwrap();
        let port = handle.port();
        // First client opens a connection and stalls (sends nothing yet).
        let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        // Second client must get an answer while the first is still silent.
        let mut fast = TcpStream::connect(("127.0.0.1", port)).unwrap();
        fast.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        fast.write_all(
            b"GET /ping HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: chrome-extension://abc\r\n\r\n",
        )
        .unwrap();
        let mut resp = String::new();
        let _ = fast.read_to_string(&mut resp);
        assert!(resp.starts_with("HTTP/1.1 200"), "got: {resp}");
        let _ = slow.write_all(b"\r\n");
        handle.stop();
    }

    #[test]
    fn outcome_title_is_brace_free_for_display() {
        assert_eq!(
            display_title("{{Attention}} {{Is}} All {{You}} Need"),
            "Attention Is All You Need"
        );
    }
}

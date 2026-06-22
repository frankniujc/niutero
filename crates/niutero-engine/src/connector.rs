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
//! 3. require an **extension `Origin`** (`*-extension://`), rejecting ordinary
//!    web origins (defeats page CSRF);
//! 4. send **no `Access-Control-Allow-*`** — a web page's script can't read our
//!    responses, while the extension's `host_permissions` fetch is unaffected.
//!
//! **Routes** (both JSON):
//! - `GET /ping` → `{app, ok, version, library}` so the extension can show
//!   whether niutero is up and which library is open.
//! - `POST /import` with `{identifier?, metadata?, tags?}` → resolve (a DOI /
//!   `arXiv:` id over the network, or scraped metadata offline), merge with
//!   skip-on-duplicate, run the import hooks, and answer `{ok, citekey, …}`.
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

use niutero_core::BibEntry;
use niutero_vault::Vault;

/// Default loopback port. Deliberately not Zotero's `23119`, so both can run
/// during a migration. The browser extension hardcodes the same value.
pub const DEFAULT_PORT: u16 = 23510;

/// Largest accepted request body (one entry's worth of metadata). Anything
/// larger gets `413` without allocation.
const MAX_BODY: usize = 64 * 1024;

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
    pub title: String,
    pub added: usize,
    pub skipped: usize,
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
    /// Called after a successful import so a UI host can refresh. No-op for CLI.
    pub on_import: Arc<dyn Fn() + Send + Sync>,
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

fn serve_loop(listener: TcpListener, cfg: ConnectorConfig, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                // One bad connection must not kill the server. Debug, not warn:
                // client-caused, and port scanners would spam warns.
                if let Err(e) = handle_connection(&mut stream, &cfg) {
                    log::debug!("connector: connection error: {e}");
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
            if !origin_ok(req.origin.as_deref()) {
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
            (cfg.on_import)();
            ("200 OK", outcome_body(&o))
        }
        Err(e) => ("200 OK", json_error(&e)),
    }
}

// ------------------------------------------------------------ import pipeline

/// Resolve one capture and apply it to `v`, then run the import hooks
/// (enrich → normalize → PDF fetch → keep-updated refresh → auto-commit, each
/// gated by config and best-effort). The network resolve runs on the caller's
/// thread (the server thread), never a UI thread.
pub fn connector_import(v: &mut Vault, req: &ImportRequest) -> Result<ImportOutcome, String> {
    // Honor the library's configured duplicate policy (Skip if unset), exactly
    // like every other import path.
    let policy = crate::default_dup_policy(v, crate::DupPolicy::Skip);
    let report = if let Some(id) = req
        .identifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        crate::import_doi(v, &identifier_to_doi(id), policy)?
    } else if let Some(meta) = &req.metadata {
        // Hold the vault lock across build + merge, the metadata path's
        // read-modify-write of references.bib — `import_doi` locks the same way,
        // and `merge_incoming`'s contract requires the caller to hold the lock.
        // Scoped to this block so the post-import hooks below can re-lock (the
        // lock is non-reentrant).
        let _lock = crate::lock_vault(v)?;
        let entry = build_entry_from_metadata(v, meta)?;
        crate::merge_incoming(v, vec![entry], policy)?
    } else {
        return Err("the capture had neither an identifier nor metadata".into());
    };

    let new_keys = report.new_keys();

    // Tags first (sidecar only) so any later hook sees a complete entry.
    if !req.tags.is_empty() && !new_keys.is_empty() {
        let adds: Vec<(String, Vec<String>)> = new_keys
            .iter()
            .map(|k| (k.clone(), req.tags.clone()))
            .collect();
        if let Err(e) = crate::set_tags_bulk(v, &adds) {
            log::warn!("connector: tagging new entries failed: {e}");
        }
    }

    if !new_keys.is_empty() {
        if let Err(e) = crate::auto_enrich(v, &new_keys) {
            log::warn!("connector: enrich skipped: {e}");
        }
        match crate::auto_normalize(v, &new_keys) {
            Ok(n) if n > 0 => log::info!("connector: normalized {n} new entr(ies)"),
            Ok(_) => {}
            Err(e) => log::warn!("connector: normalize skipped: {e}"),
        }
        match crate::auto_fetch_pdfs(v, &new_keys) {
            Ok((f, a)) if a > 0 => log::info!("connector: fetched {f}/{a} PDF(s)"),
            Ok(_) => {}
            Err(e) => log::warn!("connector: PDF fetch skipped: {e}"),
        }
    }

    if report.added > 0 {
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

    let (citekey, title) = match new_keys.first() {
        Some(k) => {
            let title = crate::show(v, k)
                .ok()
                .and_then(|view| view.fields.get("title").cloned())
                .unwrap_or_default();
            (k.clone(), title)
        }
        None => (String::new(), String::new()),
    };
    Ok(ImportOutcome {
        citekey,
        title,
        added: report.added,
        skipped: report.skipped,
    })
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

/// Build a [`BibEntry`] from scraped metadata, keyed with the library's cite-key
/// pattern (so connector entries match every other entry). Validated by the
/// caller's `merge_incoming`.
fn build_entry_from_metadata(v: &Vault, m: &ScrapedMetadata) -> Result<BibEntry, String> {
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
        _ => {}
    }
    set_if("volume", &m.volume, &mut e);
    set_if("number", &m.issue, &mut e);
    set_if("pages", &m.pages, &mut e);
    set_if("publisher", &m.publisher, &mut e);
    set_if("doi", &m.doi, &mut e);
    set_if("url", &m.url, &mut e);

    // The BASE pattern key (no uniquifying suffix) on purpose: re-capturing the
    // same page must render the same key so the dup policy can skip/rename it,
    // not slip past as a fresh entry. `merge_incoming` applies the policy.
    let base = crate::resolve_pattern(v, None).render(&e);
    e.citekey = if base.trim().is_empty() {
        "ref".to_string()
    } else {
        base
    };
    e.validate()?;
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

/// An extension `Origin` (defeats page CSRF). A missing `Origin` is allowed: the
/// extension's background fetch may omit it, and loopback already gates us.
fn origin_ok(origin: Option<&str>) -> bool {
    origin.is_none_or(origin_is_extension)
}

/// Pure: is this `Origin` an extension scheme (or the opaque `null`)?
fn origin_is_extension(value: &str) -> bool {
    let o = value.trim();
    o == "null"
        || o.starts_with("chrome-extension://")
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
    format!(
        "{{\"ok\":true,\"citekey\":{},\"title\":{},\"added\":{},\"skipped\":{}}}",
        json_str(&o.citekey),
        json_str(&o.title),
        o.added,
        o.skipped
    )
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
            on_import: Arc::new(|| {}),
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
        // Missing Host is allowed (the bind is the barrier); missing Origin too.
        assert!(host_ok(None));
        assert!(origin_ok(None));
    }

    #[test]
    fn origin_extension_accept_and_reject() {
        for o in [
            "chrome-extension://abcdefghijklmnop",
            "moz-extension://1234-5678",
            "safari-web-extension://deadbeef",
            "null",
        ] {
            assert!(origin_is_extension(o), "should accept Origin {o:?}");
        }
        for o in [
            "https://evil.example.com",
            "http://localhost:3000",
            "https://niutero.example",
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
}

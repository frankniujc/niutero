//! Browser connector — a tiny loopback HTTP server that accepts BibTeX captured
//! by a browser extension and adds it to the vault via [`crate::capture`].
//!
//! It binds `127.0.0.1` only and speaks just enough HTTP/1.1 by hand (no
//! dependency, the same spirit as shelling out elsewhere). The *extension* (the
//! browser-side JS that POSTs a page's citation here) is out of scope for this
//! crate; this is the endpoint it talks to. Everything but the actual socket
//! accept loop is pure and unit-tested, and the loop itself is covered by a
//! loopback integration test.
//!
//! **Hardening.** The mutating route (`POST /capture`) requires the per-session
//! bearer token the server was started with — loopback alone is not an
//! authorization boundary (any web page or local process can hit 127.0.0.1, so
//! without the token a malicious page could inject entries into the library).
//! No CORS-allow headers are emitted: the extension talks to us with host
//! permissions (exempt from CORS), and a random page's script must NOT be able
//! to read responses. Bodies are capped and sockets time out, so one slow or
//! abusive client can't wedge the single-threaded loop.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use niutero_vault::Vault;

/// The largest accepted request body. A capture is a page's BibTeX — a few KB;
/// half a megabyte is generous. Anything larger gets `413` without allocation.
const MAX_BODY: usize = 512 * 1024;

/// How long a client may dawdle before its socket is dropped.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(10);

/// Generate a per-session connector token (~128 bits, hex). Uses two fresh
/// [`std::collections::hash_map::RandomState`]s, each seeded from OS
/// randomness — no new dependency.
pub fn connector_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    for _ in 0..2 {
        let h = std::collections::hash_map::RandomState::new().build_hasher();
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// **Online (local).** Run the connector on `127.0.0.1:<port>`, blocking until
/// the process is killed. `POST /capture` with a BibTeX body and the session
/// `token` (as `Authorization: Bearer <token>` or `X-Niutero-Token: <token>`)
/// adds the entries (skipping cite keys that already exist); `GET /` is an
/// unauthenticated health check.
pub fn serve_connector(v: &Vault, port: u16, token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("the connector needs a non-empty session token".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    // Port only — NEVER the token.
    log::info!("connector listening on 127.0.0.1:{port}");
    serve_loop(v, &listener, token, false)
}

/// The accept loop. `once` (tests) handles a single connection then returns.
fn serve_loop(v: &Vault, listener: &TcpListener, token: &str, once: bool) -> Result<(), String> {
    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                // One bad client connection shouldn't kill the server. Debug,
                // not warn: client-caused, and port scanners would spam warns.
                if let Err(e) = handle_connection(v, &mut s, token) {
                    log::debug!("connector: connection error: {e}");
                }
            }
            Err(e) => return Err(format!("accept: {e}")),
        }
        if once {
            break;
        }
    }
    Ok(())
}

fn handle_connection(v: &Vault, stream: &mut TcpStream, token: &str) -> io::Result<()> {
    // A client that connects and never finishes its request must not wedge
    // the (single-threaded) loop.
    let _ = stream.set_read_timeout(Some(SOCKET_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
    let response = match read_request(stream) {
        Ok(req) => {
            let (status, body) = route(v, &req, token);
            http_response(status, &body)
        }
        Err(ReadError::TooLarge) => {
            http_response("413 Payload Too Large", &json_error("body too large"))
        }
        Err(ReadError::Io(e)) => {
            log::debug!("connector: malformed request: {e}");
            http_response("400 Bad Request", "{\"error\":\"malformed request\"}")
        }
    };
    stream.write_all(response.as_bytes())
}

/// Dispatch a parsed request to a (status line, JSON body).
fn route(v: &Vault, req: &Request, token: &str) -> (&'static str, String) {
    let mut capture_note = String::new();
    let (status, body) = match (req.method.as_str(), req.path.as_str()) {
        ("OPTIONS", _) => ("204 No Content", String::new()),
        ("POST", "/capture") => {
            if !token_matches(req.auth.as_deref(), token) {
                (
                    "401 Unauthorized",
                    json_error("missing or wrong connector token"),
                )
            } else {
                match crate::capture(v, &req.body) {
                    Ok(r) => {
                        // A capture mutates references.bib, but it happens inside this
                        // server loop — never on the CLI's run()-level path — so the
                        // post-mutation work (keep-updated exports, the opt-in
                        // workflow hooks) must run here. All best-effort: a stale
                        // mirror or failed hook must not fail the capture (the HTTP
                        // body is the capture's machine output).
                        if r.added > 0 {
                            refresh_after_capture(v);
                            run_capture_hooks(v, &r);
                        }
                        capture_note = format!(" (added {}, skipped {})", r.added, r.skipped);
                        (
                            "200 OK",
                            format!("{{\"added\":{},\"skipped\":{}}}", r.added, r.skipped),
                        )
                    }
                    Err(e) => ("400 Bad Request", json_error(&e)),
                }
            }
        }
        ("GET", "/" | "/health") => ("200 OK", "{\"service\":\"niutero-connector\"}".into()),
        _ => ("404 Not Found", json_error("unknown endpoint")),
    };
    // NEVER log req.auth (the presented token) or req.body (user content).
    log::debug!(
        "connector: {} {} -> {status}{capture_note}",
        req.method,
        req.path
    );
    (status, body)
}

/// Constant-time-ish token comparison (no early exit on the first wrong
/// byte). An empty expected token never matches — [`serve_connector`] refuses
/// to start with one, and this guards the invariant in depth.
fn token_matches(got: Option<&str>, want: &str) -> bool {
    match got {
        None => false,
        Some(got) => {
            !want.is_empty()
                && got.len() == want.len()
                && got
                    .bytes()
                    .zip(want.bytes())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                    == 0
        }
    }
}

/// The same opt-in post-import hooks every other import path runs (CLI
/// `import`, GUI imports): PDF auto-fetch, enrich-on-import, then the
/// auto-commit — each a no-op without its pref, all best-effort.
fn run_capture_hooks(v: &Vault, r: &crate::ImportReport) {
    let keys = r.new_keys();
    match crate::auto_fetch_pdfs(v, &keys) {
        Ok((fetched, attempted)) if attempted > 0 => {
            log::info!("capture: auto-fetched {fetched}/{attempted} PDF(s)");
        }
        Ok(_) => {}
        Err(e) => log::warn!("capture: PDF auto-fetch skipped: {e}"),
    }
    match crate::auto_enrich(v, &keys) {
        Ok((filled, attempted)) if attempted > 0 => {
            log::info!("capture: auto-enriched {filled}/{attempted} entr(ies)");
        }
        Ok(_) => {}
        Err(e) => log::warn!("capture: auto-enrich skipped: {e}"),
    }
    match crate::auto_commit_if_enabled(v) {
        Ok(Some(msg)) => log::info!("capture: auto-committed: {msg}"),
        Ok(None) => {}
        Err(e) => log::warn!("capture: auto-commit failed: {e}"),
    }
}

/// Re-export keep-updated targets after a capture (best-effort; logged only).
fn refresh_after_capture(v: &Vault) {
    match crate::refresh_exports(v) {
        Ok(outcomes) => {
            for o in outcomes {
                match o.error {
                    None => log::info!("keep-updated: {} entr(ies) → {}", o.count, o.out.display()),
                    Some(e) => {
                        log::warn!("keep-updated export to {} failed: {e}", o.out.display())
                    }
                }
            }
        }
        Err(e) => log::warn!("keep-updated export skipped: {e}"),
    }
}

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

fn json_error(msg: &str) -> String {
    // Minimal JSON string escaping for the message.
    let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{{\"error\":\"{escaped}\"}}")
}

/// A parsed HTTP request: just what the connector needs.
struct Request {
    method: String,
    path: String,
    body: String,
    /// The presented session token, from `Authorization: Bearer …` or
    /// `X-Niutero-Token: …`.
    auth: Option<String>,
}

enum ReadError {
    /// Declared `Content-Length` over [`MAX_BODY`] — refused before any
    /// allocation, so a hostile length can't OOM the process.
    TooLarge,
    /// Logged at debug in the 400 branch; the response is a generic 400 either way.
    Io(io::Error),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        ReadError::Io(e)
    }
}

/// Read one HTTP/1.1 request: the request line + headers (to find
/// `Content-Length` and the auth token) then exactly that many body bytes.
fn read_request(stream: &TcpStream) -> Result<Request, ReadError> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break; // end of headers (or connection closed)
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
        auth: auth_token(&head),
    })
}

/// The presented token from `Authorization: Bearer …` / `X-Niutero-Token: …`.
/// Pure.
fn auth_token(head: &str) -> Option<String> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        let (k, v) = (k.trim(), v.trim());
        if k.eq_ignore_ascii_case("x-niutero-token") {
            Some(v.to_string())
        } else if k.eq_ignore_ascii_case("authorization") {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .map(|t| t.trim().to_string())
        } else {
            None
        }
    })
}

/// Method and path from the first line ("POST /capture HTTP/1.1"). Pure.
fn request_line(head: &str) -> (String, String) {
    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    (method, path)
}

/// The `Content-Length` header value (0 if absent/invalid). Pure.
fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// Hold the crate-wide env lock and point `$NIUTERO_REGISTRY` at a fresh
    /// temp file, so a capture's keep-updated refresh reads an isolated registry
    /// instead of the real machine one.
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

    /// Serve one request and return the raw response.
    fn one_shot(v: Vault, token: &'static str, request: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || serve_loop(&v, &listener, token, true));
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        handle.join().unwrap().unwrap();
        response
    }

    #[test]
    fn parses_request_line_content_length_and_auth() {
        let head = "POST /capture HTTP/1.1\r\nHost: x\r\nContent-Length: 42\r\n";
        assert_eq!(request_line(head), ("POST".into(), "/capture".into()));
        assert_eq!(content_length(head), 42);
        // case-insensitive header name; absent → 0
        assert_eq!(content_length("GET / HTTP/1.1\r\ncontent-length: 7\r\n"), 7);
        assert_eq!(content_length("GET / HTTP/1.1\r\n"), 0);
        // Both token spellings parse; Bearer prefix is case-tolerant.
        assert_eq!(
            auth_token("POST / HTTP/1.1\r\nAuthorization: Bearer tok123\r\n").as_deref(),
            Some("tok123")
        );
        assert_eq!(
            auth_token("POST / HTTP/1.1\r\nx-niutero-token: tok123\r\n").as_deref(),
            Some("tok123")
        );
        assert_eq!(auth_token("POST / HTTP/1.1\r\nHost: x\r\n"), None);
        // Comparison is exact (and never matches an absent token).
        assert!(token_matches(Some("tok123"), "tok123"));
        assert!(!token_matches(Some("tok124"), "tok123"));
        assert!(!token_matches(Some(""), ""));
        assert!(!token_matches(None, "tok123"));
    }

    #[test]
    fn connector_token_is_long_and_unique() {
        let (a, b) = (connector_token(), connector_token());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn capture_over_a_loopback_socket_adds_the_entry() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let body = "@article{cap, title={Captured}, author={A, B}, year={2024}}";
        let response = one_shot(
            v,
            "tok123",
            format!(
                "POST /capture HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer tok123\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );

        assert!(response.starts_with("HTTP/1.1 200 OK"), "got: {response}");
        assert!(response.contains("\"added\":1"), "got: {response}");
        // No CORS-allow headers: a web page's script must not read responses.
        assert!(
            !response.contains("Access-Control-Allow"),
            "got: {response}"
        );

        // The captured entry is really in the vault.
        let reopened = crate::open(dir.path()).unwrap();
        assert!(crate::show(&reopened, "cap").is_ok());
    }

    #[test]
    fn capture_without_the_token_is_refused_and_adds_nothing() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let body = "@article{evil, title={Injected}}";

        // Missing token.
        let response = one_shot(
            crate::open(dir.path()).unwrap(),
            "tok123",
            format!(
                "POST /capture HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(response.starts_with("HTTP/1.1 401"), "got: {response}");

        // Wrong token.
        let response = one_shot(
            v,
            "tok123",
            format!(
                "POST /capture HTTP/1.1\r\nHost: localhost\r\nX-Niutero-Token: wrong\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(response.starts_with("HTTP/1.1 401"), "got: {response}");

        // Nothing was added either time.
        let reopened = crate::open(dir.path()).unwrap();
        assert!(crate::show(&reopened, "evil").is_err());
    }

    #[test]
    fn oversized_content_length_is_413_not_an_allocation() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        // A hostile declared length (way past the cap, no actual body) must be
        // refused up front — never pre-allocated or waited for.
        let response = one_shot(
            v,
            "tok123",
            "POST /capture HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer tok123\r\n\
             Content-Length: 999999999999\r\n\r\n"
                .to_string(),
        );
        assert!(response.starts_with("HTTP/1.1 413"), "got: {response}");
    }

    #[test]
    fn health_check_needs_no_token() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let response = one_shot(
            v,
            "tok123",
            "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n".to_string(),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "got: {response}");
        assert!(response.contains("niutero-connector"));
    }

    #[test]
    fn capture_refreshes_keep_updated_export_targets() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let mirror_dir = tempfile::tempdir().unwrap();
        let mirror = mirror_dir.path().join("mirror.bib");
        crate::export_target_add(&v, &mirror, None).unwrap();

        let body = "@article{cap, title={Captured}, year={2024}}";
        let response = one_shot(
            v,
            "tok123",
            format!(
                "POST /capture HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer tok123\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"), "got: {response}");

        // The capture happens inside the server loop (never the CLI run()-level
        // path), so the connector itself must refresh the mirror — verify it did.
        let mirrored = std::fs::read_to_string(&mirror).unwrap();
        assert!(
            mirrored.contains("@article{cap"),
            "mirror stale: {mirrored}"
        );
    }
}

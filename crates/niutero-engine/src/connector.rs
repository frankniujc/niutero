//! Browser connector — a tiny loopback HTTP server that accepts BibTeX captured
//! by a browser extension and adds it to the vault via [`crate::capture`].
//!
//! It binds `127.0.0.1` only and speaks just enough HTTP/1.1 by hand (no
//! dependency, the same spirit as shelling out elsewhere). The *extension* (the
//! browser-side JS that POSTs a page's citation here) is out of scope for this
//! crate; this is the endpoint it talks to. Everything but the actual socket
//! accept loop is pure and unit-tested, and the loop itself is covered by a
//! loopback integration test.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use niutero_vault::Vault;

/// **Online (local).** Run the connector on `127.0.0.1:<port>`, blocking until
/// the process is killed. `POST /capture` with a BibTeX body adds the entries
/// (skipping cite keys that already exist); `GET /` is a health check. CORS is
/// open so an extension on any page can reach the loopback endpoint.
pub fn serve_connector(v: &Vault, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    serve_loop(v, &listener, false)
}

/// The accept loop. `once` (tests) handles a single connection then returns.
fn serve_loop(v: &Vault, listener: &TcpListener, once: bool) -> Result<(), String> {
    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                // One bad client connection shouldn't kill the server.
                let _ = handle_connection(v, &mut s);
            }
            Err(e) => return Err(format!("accept: {e}")),
        }
        if once {
            break;
        }
    }
    Ok(())
}

fn handle_connection(v: &Vault, stream: &mut TcpStream) -> io::Result<()> {
    let response = match read_request(stream) {
        Ok(req) => {
            let (status, body) = route(v, &req);
            http_response(status, &body)
        }
        Err(_) => http_response("400 Bad Request", "{\"error\":\"malformed request\"}"),
    };
    stream.write_all(response.as_bytes())
}

/// Dispatch a parsed request to a (status line, JSON body).
fn route(v: &Vault, req: &Request) -> (&'static str, String) {
    match (req.method.as_str(), req.path.as_str()) {
        // CORS preflight.
        ("OPTIONS", _) => ("204 No Content", String::new()),
        ("POST", "/capture") => match crate::capture(v, &req.body) {
            Ok(r) => {
                // A capture mutates references.bib, but it happens inside this
                // server loop — never on the CLI's run()-level path — so the
                // keep-updated export (#45) won't fire there. Trigger it here,
                // best-effort: a stale mirror shouldn't fail the capture. Notes
                // go to stderr (the HTTP body is the capture's machine output).
                if r.added > 0 {
                    refresh_after_capture(v);
                }
                (
                    "200 OK",
                    format!("{{\"added\":{},\"skipped\":{}}}", r.added, r.skipped),
                )
            }
            Err(e) => ("400 Bad Request", json_error(&e)),
        },
        ("GET", "/" | "/health") => ("200 OK", "{\"service\":\"niutero-connector\"}".into()),
        _ => ("404 Not Found", json_error("unknown endpoint")),
    }
}

/// Re-export keep-updated targets after a capture (best-effort; stderr only).
fn refresh_after_capture(v: &Vault) {
    match crate::refresh_exports(v) {
        Ok(outcomes) => {
            for o in outcomes {
                match o.error {
                    None => eprintln!("  ↻ {} entr(ies) → {}", o.count, o.out.display()),
                    Some(e) => {
                        eprintln!(
                            "warning: keep-updated export to {} failed: {e}",
                            o.out.display()
                        )
                    }
                }
            }
        }
        Err(e) => eprintln!("warning: keep-updated export skipped: {e}"),
    }
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
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
}

/// Read one HTTP/1.1 request: the request line + headers (to find
/// `Content-Length`) then exactly that many body bytes.
fn read_request(stream: &TcpStream) -> io::Result<Request> {
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
    let mut body = vec![0u8; content_length(&head)];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
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

    #[test]
    fn parses_request_line_and_content_length() {
        let head = "POST /capture HTTP/1.1\r\nHost: x\r\nContent-Length: 42\r\n";
        assert_eq!(request_line(head), ("POST".into(), "/capture".into()));
        assert_eq!(content_length(head), 42);
        // case-insensitive header name; absent → 0
        assert_eq!(content_length("GET / HTTP/1.1\r\ncontent-length: 7\r\n"), 7);
        assert_eq!(content_length("GET / HTTP/1.1\r\n"), 0);
    }

    #[test]
    fn capture_over_a_loopback_socket_adds_the_entry() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // Serve exactly one request on a worker thread.
        let handle = thread::spawn(move || serve_loop(&v, &listener, true));

        let body = "@article{cap, title={Captured}, author={A, B}, year={2024}}";
        let mut client = TcpStream::connect(addr).unwrap();
        let request = format!(
            "POST /capture HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        client.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        handle.join().unwrap().unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK"), "got: {response}");
        assert!(response.contains("\"added\":1"), "got: {response}");
        assert!(response.contains("Access-Control-Allow-Origin: *"));

        // The captured entry is really in the vault.
        let reopened = crate::open(dir.path()).unwrap();
        assert!(crate::show(&reopened, "cap").is_ok());
    }

    #[test]
    fn capture_refreshes_keep_updated_export_targets() {
        let _env = isolated_registry();
        let dir = tempfile::tempdir().unwrap();
        let v = crate::init(dir.path()).unwrap();
        let mirror_dir = tempfile::tempdir().unwrap();
        let mirror = mirror_dir.path().join("mirror.bib");
        crate::export_target_add(&v, &mirror, None).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || serve_loop(&v, &listener, true));

        let body = "@article{cap, title={Captured}, year={2024}}";
        let mut client = TcpStream::connect(addr).unwrap();
        let request = format!(
            "POST /capture HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        client.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        handle.join().unwrap().unwrap();

        // The capture happens inside the server loop (never the CLI run()-level
        // path), so the connector itself must refresh the mirror — verify it did.
        let mirrored = std::fs::read_to_string(&mirror).unwrap();
        assert!(
            mirrored.contains("@article{cap"),
            "mirror stale: {mirrored}"
        );
    }
}

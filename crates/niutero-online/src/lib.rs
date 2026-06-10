//! niutero-online — optional online lookups by shelling out to the system
//! `curl` (no HTTP/TLS crate, the same way `niutero-sync` uses `git`).
//!
//! **Everything here needs network access.** It stays strictly off niutero's
//! offline base path: the engine calls it only for the explicit online
//! commands, and the core works fully with it unused. The URL/spec building and
//! response handling are pure and tested; only the `curl` call itself can't be
//! exercised without connectivity.

use std::process::Command;

/// Is a usable `curl` on PATH?
pub fn curl_available() -> bool {
    run(&["--version"]).is_ok_and(|o| o.status.success())
}

/// Fetch an entry's canonical BibTeX from its DOI via doi.org content
/// negotiation (`Accept: application/x-bibtex`). Needs network access.
pub fn fetch_doi_bibtex(doi: &str) -> Result<String, String> {
    let url = doi_url(doi);
    let body = ok(&[
        "-fsSL",
        "--max-time",
        "30",
        "-H",
        "Accept: application/x-bibtex",
        &url,
    ])?;
    if body.trim().is_empty() {
        return Err(format!("doi.org returned no BibTeX for {doi}"));
    }
    Ok(body)
}

/// Download `url` to `dest` (following redirects). Needs network access.
pub fn fetch_to_file(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let dest = dest.to_str().ok_or("destination path is not valid UTF-8")?;
    ok(&["-fsSL", "--max-time", "120", "-o", dest, url]).map(|_| ())
}

/// Ask Claude (Anthropic Messages API) for a completion; returns the text.
/// Reads the API key from `$ANTHROPIC_API_KEY`. The key and request body are
/// passed via a temp `curl` config file (`-K`), never on the command line, so
/// the key isn't exposed in the process list. Needs network access.
pub fn anthropic_text(model: &str, system: &str, user: &str) -> Result<String, String> {
    let key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "set ANTHROPIC_API_KEY to use the AI features".to_string())?;
    anthropic_text_with(&key, model, 512, system, user)
}

/// Like [`anthropic_text`] but with an explicit key and `max_tokens` — so the
/// engine can drive it from the user's stored AI config. The key goes to curl
/// as a `-K -` config on **stdin** (never argv, never on disk); only the
/// request body touches disk, as a random-named 0600 temp file.
pub fn anthropic_text_with(
    key: &str,
    model: &str,
    max_tokens: u32,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("no API key configured".into());
    }
    if key.chars().any(|c| c.is_control() || c == '"' || c == '\\') {
        // Quotes/backslashes/control chars would corrupt the quoted curl config
        // line (or inject directives); no real API key contains them.
        return Err("the API key contains characters that aren't allowed in a key".into());
    }
    let body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    })
    .to_string();

    // The body goes via a random-named temp file (0600 on Unix, auto-deleted on
    // drop even if the call errors); the config — the only part holding the
    // key — is fed to `-K -` on stdin so the secret never lands on disk.
    let mut body_file = tempfile::Builder::new()
        .prefix("niutero-llm-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| format!("create request temp file: {e}"))?;
    std::io::Write::write_all(&mut body_file, body.as_bytes())
        .map_err(|e| format!("write request body: {e}"))?;
    let cfg = curl_llm_cfg(key, body_file.path());

    // No `-f`: on an HTTP error we want the API's JSON error body so
    // `extract_text` can surface its message instead of an opaque curl exit.
    let raw = ok_with_stdin(&["-sSL", "--max-time", "60", "-K", "-"], &cfg)?;
    drop(body_file); // deletes the temp file
    extract_text(&raw)
}

/// Build the `-K` curl config for an LLM call. Pure.
///
/// Inside curl's double-quoted config values backslash sequences are
/// *unescaped* (`\t` → TAB, a lone `\x` drops the backslash), which mangles
/// Windows paths — so the body path is emitted with forward slashes, which
/// curl accepts on every platform and which are inert inside the quotes.
fn curl_llm_cfg(key: &str, body_path: &std::path::Path) -> String {
    let path = fwd_slashes(body_path);
    format!(
        "url = \"https://api.anthropic.com/v1/messages\"\n\
         header = \"content-type: application/json\"\n\
         header = \"anthropic-version: 2023-06-01\"\n\
         header = \"x-api-key: {key}\"\n\
         data = \"@{path}\"\n"
    )
}

/// Pull the answer text out of a Messages API response. Pure.
///
/// Surfaces the API's own `error.message` for error bodies (reachable now that
/// the call drops `-f`), and flags responses truncated at the token limit.
fn extract_text(raw: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("parse API response: {e}"))?;
    if value["type"] == "error" {
        let kind = value["error"]["type"].as_str().unwrap_or("error");
        let msg = value["error"]["message"]
            .as_str()
            .unwrap_or("unknown API error");
        return Err(format!("API error ({kind}): {msg}"));
    }
    let stop_reason = value["stop_reason"].as_str().unwrap_or("");
    let text = value["content"].as_array().and_then(|blocks| {
        blocks
            .iter()
            .find_map(|b| (b["type"] == "text").then(|| b["text"].as_str()).flatten())
    });
    match text {
        Some(t) if stop_reason == "max_tokens" => {
            Ok(format!("{t}\n[answer truncated at the token limit]"))
        }
        Some(t) => Ok(t.to_string()),
        None if stop_reason == "max_tokens" => Err(
            "the response hit the token limit before any text was produced (raise max_tokens \
             or pick a non-thinking model)"
                .into(),
        ),
        None => Err("no text in the API response".into()),
    }
}

// ------------------------------------------------- HuggingFace (PDF sync)
//
// PDFs sync to a private HF *dataset* repo via three plain Hub endpoints:
// create-repo, the NDJSON commit API (upload), and `resolve/` (download).
// Same transport discipline as the LLM path: the token rides a `-K -` curl
// config on stdin — never argv, never disk.

/// Is `repo` a plausible `user/repo` dataset id? Pure.
fn valid_hf_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), None) if !a.is_empty() && !b.is_empty()
    ) && repo
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Reject tokens that would corrupt (or inject directives into) the quoted
/// curl-config line. Real HF tokens (`hf_…`) never contain these.
fn check_hf_inputs(token: &str, repo: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("no HuggingFace token configured".into());
    }
    if token
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\\')
    {
        return Err("the HuggingFace token contains characters that aren't allowed".into());
    }
    if !valid_hf_repo(repo) {
        return Err(format!(
            "'{repo}' isn't a valid HF dataset id (want user/repo)"
        ));
    }
    Ok(())
}

/// **Online (HF).** Create the dataset repo, private. Idempotent: an
/// already-exists answer reports success.
pub fn hf_create_dataset(token: &str, repo: &str) -> Result<String, String> {
    let token = token.trim();
    check_hf_inputs(token, repo)?;
    let body = serde_json::json!({ "type": "dataset", "name": repo, "private": true }).to_string();
    let mut body_file = tempfile::Builder::new()
        .prefix("niutero-hf-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| format!("create request temp file: {e}"))?;
    std::io::Write::write_all(&mut body_file, body.as_bytes())
        .map_err(|e| format!("write request body: {e}"))?;
    let cfg = format!(
        "url = \"https://huggingface.co/api/repos/create\"\n\
         header = \"authorization: Bearer {token}\"\n\
         header = \"content-type: application/json\"\n\
         data = \"@{}\"\n",
        fwd_slashes(body_file.path())
    );
    let raw = ok_with_stdin(&["-sSL", "--max-time", "60", "-K", "-"], &cfg)?;
    drop(body_file);
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse HF response: {e}"))?;
    if let Some(url) = value["url"].as_str() {
        return Ok(format!("created {url}"));
    }
    let err = value["error"].as_str().unwrap_or("unknown HF error");
    if err.to_ascii_lowercase().contains("already") {
        Ok(format!("dataset {repo} already exists"))
    } else {
        Err(format!("HF create repo: {err}"))
    }
}

/// **Online (HF).** Upload `file` to `remote_path` in the dataset via the
/// NDJSON commit API.
pub fn hf_upload(
    token: &str,
    repo: &str,
    remote_path: &str,
    file: &std::path::Path,
) -> Result<(), String> {
    let token = token.trim();
    check_hf_inputs(token, repo)?;
    let bytes = std::fs::read(file).map_err(|e| format!("read {}: {e}", file.display()))?;
    let ndjson = hf_commit_ndjson(
        &format!("niutero: update {remote_path}"),
        remote_path,
        &base64(&bytes),
    );
    let mut body_file = tempfile::Builder::new()
        .prefix("niutero-hf-")
        .suffix(".ndjson")
        .tempfile()
        .map_err(|e| format!("create request temp file: {e}"))?;
    std::io::Write::write_all(&mut body_file, ndjson.as_bytes())
        .map_err(|e| format!("write request body: {e}"))?;
    let cfg = format!(
        "url = \"https://huggingface.co/api/datasets/{repo}/commit/main\"\n\
         header = \"authorization: Bearer {token}\"\n\
         header = \"content-type: application/x-ndjson\"\n\
         data = \"@{}\"\n",
        fwd_slashes(body_file.path())
    );
    let raw = ok_with_stdin(&["-sSL", "--max-time", "300", "-K", "-"], &cfg)?;
    drop(body_file);
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse HF response: {e}"))?;
    if value["commitOid"].is_string() || value["commitUrl"].is_string() {
        Ok(())
    } else {
        Err(format!(
            "HF upload: {}",
            value["error"].as_str().unwrap_or("unknown HF error")
        ))
    }
}

/// **Online (HF).** Download `remote_path` from the dataset to `dest`.
/// `--fail` here is deliberate (unlike the LLM call): an HTTP error page must
/// never be written into the destination file as if it were the PDF.
pub fn hf_download(
    token: &str,
    repo: &str,
    remote_path: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    let token = token.trim();
    check_hf_inputs(token, repo)?;
    let cfg = format!(
        "url = \"https://huggingface.co/datasets/{repo}/resolve/main/{remote_path}\"\n\
         header = \"authorization: Bearer {token}\"\n\
         location\n\
         fail\n\
         output = \"{}\"\n",
        fwd_slashes(dest)
    );
    ok_with_stdin(&["-sS", "--max-time", "300", "-K", "-"], &cfg)
        .map(|_| ())
        .map_err(|e| format!("HF download failed ({e}) — check the repo, path, and token"))
}

/// A path as a forward-slashed string for a quoted curl-config value (curl
/// unescapes backslash sequences inside double quotes). Pure.
fn fwd_slashes(p: &std::path::Path) -> String {
    p.display().to_string().replace('\\', "/")
}

/// The two-line NDJSON payload for a single-file HF commit. Pure; built with
/// `serde_json` so paths/summaries are correctly escaped.
fn hf_commit_ndjson(summary: &str, remote_path: &str, content_b64: &str) -> String {
    let header = serde_json::json!({ "key": "header", "value": { "summary": summary } });
    let file = serde_json::json!({
        "key": "file",
        "value": { "content": content_b64, "path": remote_path, "encoding": "base64" }
    });
    format!("{header}\n{file}\n")
}

/// Standard base64 (RFC 4648, with padding). ~15 lines beats a new dependency
/// for one call site. Pure.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Normalize a DOI — a bare DOI, a `doi:`-prefixed one, or a doi.org URL — to a
/// canonical `https://doi.org/<doi>` URL. A non-doi.org `http(s)` URL is kept
/// as-is (so a direct BibTeX URL also works).
pub fn doi_url(doi: &str) -> String {
    let d = doi.trim();
    let d = d.strip_prefix("doi:").unwrap_or(d).trim();
    if let Some(rest) = d
        .strip_prefix("https://doi.org/")
        .or_else(|| d.strip_prefix("http://doi.org/"))
    {
        format!("https://doi.org/{}", rest.trim_start_matches('/'))
    } else if d.starts_with("http://") || d.starts_with("https://") {
        d.to_string()
    } else {
        format!("https://doi.org/{d}")
    }
}

// ----------------------------------------------------------------- helpers

fn run(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("curl")
        .args(args)
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "curl not found on PATH (install curl to use online features)".to_string()
            }
            _ => format!("failed to run curl: {e}"),
        })
}

fn ok(args: &[&str]) -> Result<String, String> {
    let out = run(args)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "curl failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Run curl with `input` piped to stdin (for `-K -` configs that must never
/// touch disk or argv). The input is tiny, so no pipe-deadlock risk.
fn ok_with_stdin(args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("curl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "curl not found on PATH (install curl to use online features)".to_string()
            }
            _ => format!("failed to run curl: {e}"),
        })?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input.as_bytes())
        .map_err(|e| format!("write curl config: {e}"))?; // drop closes the pipe
    let out = child
        .wait_with_output()
        .map_err(|e| format!("failed to run curl: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "curl failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doi_url_normalizes_the_common_forms() {
        assert_eq!(
            doi_url("10.1145/3292500"),
            "https://doi.org/10.1145/3292500"
        );
        assert_eq!(doi_url("doi:10.1/x"), "https://doi.org/10.1/x");
        assert_eq!(doi_url("  10.1/x  "), "https://doi.org/10.1/x");
        assert_eq!(doi_url("https://doi.org/10.1/x"), "https://doi.org/10.1/x");
        assert_eq!(doi_url("http://doi.org/10.1/x"), "https://doi.org/10.1/x");
        // a direct (non-doi.org) URL is left alone
        assert_eq!(
            doi_url("https://aclanthology.org/N19-1423.bib"),
            "https://aclanthology.org/N19-1423.bib"
        );
    }

    #[test]
    fn curl_llm_cfg_forward_slashes_windows_paths() {
        // The worst case: `\T` would become a TAB and `\n` a newline inside
        // curl's double-quoted config value. Spaces must survive as-is.
        let p = std::path::Path::new(r"C:\Users\frank y\AppData\Local\Temp\niutero-llm-1.json");
        let cfg = curl_llm_cfg("sk-ant-test123", p);
        let data_line = cfg.lines().find(|l| l.starts_with("data")).unwrap();
        assert_eq!(
            data_line,
            "data = \"@C:/Users/frank y/AppData/Local/Temp/niutero-llm-1.json\""
        );
        assert!(!data_line.contains('\\'));
        assert!(cfg.contains("header = \"x-api-key: sk-ant-test123\""));
        // A Unix path passes through unchanged.
        let cfg = curl_llm_cfg("k", std::path::Path::new("/tmp/niutero-llm-1.json"));
        assert!(cfg.contains("data = \"@/tmp/niutero-llm-1.json\""));
    }

    #[test]
    fn anthropic_text_with_rejects_bad_keys_before_any_io() {
        // Empty / whitespace-only.
        assert!(anthropic_text_with("  ", "m", 16, "s", "u").is_err());
        // Quote, backslash, or control chars would corrupt the curl config.
        for bad in ["sk\"x", "sk\\x", "sk\nx"] {
            let err = anthropic_text_with(bad, "m", 16, "s", "u").unwrap_err();
            assert!(err.contains("aren't allowed"), "{bad}: {err}");
        }
    }

    #[test]
    fn extract_text_finds_the_first_text_block() {
        let raw = r#"{"content":[{"type":"thinking","thinking":"…"},{"type":"text","text":"hello"}],"stop_reason":"end_turn"}"#;
        assert_eq!(extract_text(raw).unwrap(), "hello");
    }

    #[test]
    fn extract_text_surfaces_api_error_bodies() {
        let raw = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let err = extract_text(raw).unwrap_err();
        assert!(err.contains("authentication_error"));
        assert!(err.contains("invalid x-api-key"));
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Binary-ish input (a PDF magic header) round-trips the table edges.
        assert_eq!(base64(b"%PDF-1.4\x00\xff"), "JVBERi0xLjQA/w==");
    }

    #[test]
    fn hf_repo_and_input_validation() {
        assert!(valid_hf_repo("frank/papers-pdfs"));
        assert!(valid_hf_repo("Org-1/data.set_v2"));
        for bad in ["", "no-slash", "a/b/c", "/x", "x/", "user/re po", "u/r\"e"] {
            assert!(!valid_hf_repo(bad), "{bad}");
        }
        assert!(check_hf_inputs("hf_tok", "user/repo").is_ok());
        assert!(check_hf_inputs("  ", "user/repo").is_err());
        assert!(check_hf_inputs("hf\"tok", "user/repo")
            .unwrap_err()
            .contains("aren't allowed"));
        assert!(check_hf_inputs("hf_tok", "nope").is_err());
    }

    #[test]
    fn hf_commit_ndjson_is_two_escaped_lines() {
        let nd = hf_commit_ndjson("niutero: update pdfs/a.pdf", "pdfs/a.pdf", "QUJD");
        let lines: Vec<&str> = nd.trim_end().lines().collect();
        assert_eq!(lines.len(), 2);
        let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let file: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(header["key"], "header");
        assert_eq!(file["value"]["path"], "pdfs/a.pdf");
        assert_eq!(file["value"]["encoding"], "base64");
        assert_eq!(file["value"]["content"], "QUJD");
        // A summary with a quote must come out JSON-escaped, not corrupting.
        let nd = hf_commit_ndjson("say \"hi\"", "p", "x");
        assert!(serde_json::from_str::<serde_json::Value>(nd.lines().next().unwrap()).is_ok());
    }

    #[test]
    fn hf_calls_reject_bad_inputs_before_any_io() {
        assert!(hf_create_dataset("", "user/repo").is_err());
        assert!(hf_create_dataset("hf_x", "bad id").is_err());
        assert!(hf_upload("hf_x", "no-slash", "p", std::path::Path::new("x")).is_err());
        assert!(hf_download("hf\"x", "user/repo", "p", std::path::Path::new("x")).is_err());
    }

    #[test]
    fn extract_text_handles_truncation_and_empty_content() {
        // Truncated but with text: keep the text, flag the truncation.
        let raw = r#"{"content":[{"type":"text","text":"partial"}],"stop_reason":"max_tokens"}"#;
        let out = extract_text(raw).unwrap();
        assert!(out.starts_with("partial"));
        assert!(out.contains("truncated"));
        // Token budget eaten before any text (e.g. a thinking-only response).
        let raw = r#"{"content":[{"type":"thinking","thinking":"…"}],"stop_reason":"max_tokens"}"#;
        assert!(extract_text(raw).unwrap_err().contains("token limit"));
        // No content at all: clean error, no panic.
        assert!(extract_text(r#"{"content":[]}"#).is_err());
    }
}

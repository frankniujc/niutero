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
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 512,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    })
    .to_string();

    // Pass secrets/body via a temp -K config so nothing lands in argv.
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let body_path = dir.join(format!("niutero-llm-{pid}.json"));
    let cfg_path = dir.join(format!("niutero-llm-{pid}.cfg"));
    std::fs::write(&body_path, &body).map_err(|e| format!("write request body: {e}"))?;
    let cfg = format!(
        "url = \"https://api.anthropic.com/v1/messages\"\n\
         header = \"content-type: application/json\"\n\
         header = \"anthropic-version: 2023-06-01\"\n\
         header = \"x-api-key: {key}\"\n\
         data = \"@{}\"\n",
        body_path.display()
    );
    std::fs::write(&cfg_path, cfg).map_err(|e| format!("write curl config: {e}"))?;

    let result = ok(&[
        "-fsSL",
        "--max-time",
        "60",
        "-K",
        cfg_path.to_str().unwrap_or_default(),
    ]);
    let _ = std::fs::remove_file(&cfg_path);
    let _ = std::fs::remove_file(&body_path);

    let raw = result?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse API response: {e}"))?;
    value["content"]
        .as_array()
        .and_then(|blocks| {
            blocks
                .iter()
                .find_map(|b| (b["type"] == "text").then(|| b["text"].as_str()).flatten())
        })
        .map(str::to_string)
        .ok_or_else(|| "no text in the API response".to_string())
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
}

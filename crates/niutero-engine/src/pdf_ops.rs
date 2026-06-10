//! PDF sync + auto-fetch — the operations behind Settings → PDF attachments.
//!
//! Local attach/fetch/open live in `lib.rs` (`attach_pdf` / `fetch_pdf` /
//! `pdf_path`); this module adds the machine-local prefs (HF dataset repo,
//! auto-fetch toggle, account token) and the HuggingFace push/pull/create
//! operations over `niutero-online`. Everything network-touching stays off the
//! base path: with no prefs configured, nothing here makes a call.
//!
//! Binaries never enter the `.bib` or git (`pdfs/` is git-ignored); the HF
//! token is machine-local registry state, like the AI key.

use std::path::PathBuf;

use niutero_vault::registry::with_registry_mut;
use niutero_vault::{PdfPrefs, Registry, Vault};

/// This machine's PDF prefs for `v` (HF repo + auto-fetch). Default when the
/// vault has none recorded.
pub fn pdf_prefs(v: &Vault) -> Result<PdfPrefs, String> {
    Ok(Registry::load()
        .map_err(|e| format!("read PDF prefs: {e}"))?
        .get(&v.root)
        .map(|r| r.pdf.clone())
        .unwrap_or_default())
}

/// Update this machine's PDF prefs for `v`, with the read-modify-write inside
/// the registry's exclusive lock (the same discipline as the AI config).
/// Returns the prefs as saved.
pub fn update_pdf_prefs(v: &Vault, f: impl FnOnce(&mut PdfPrefs)) -> Result<PdfPrefs, String> {
    let root = v.root.clone();
    with_registry_mut(|reg| {
        let rec = reg.entry_mut(&root);
        f(&mut rec.pdf);
        rec.pdf.clone()
    })
    .map_err(|e| format!("save PDF prefs: {e}"))
}

/// Whether a HuggingFace token is configured (the token itself is never
/// exposed through the engine API).
pub fn hf_token_set() -> Result<bool, String> {
    Ok(!Registry::load()
        .map_err(|e| format!("read HF token: {e}"))?
        .hf_token
        .trim()
        .is_empty())
}

/// Store (or clear, with an empty string) the machine-local HF token, under
/// the registry lock.
pub fn set_hf_token(token: &str) -> Result<(), String> {
    let token = token.trim().to_string();
    with_registry_mut(|reg| reg.hf_token = token).map_err(|e| format!("save HF token: {e}"))
}

/// Resolve the (token, repo) an HF call needs, with actionable errors naming
/// both the CLI command and the Settings page.
fn resolve_hf(v: &Vault) -> Result<(String, String), String> {
    let reg = Registry::load().map_err(|e| format!("read HF config: {e}"))?;
    let token = reg.hf_token.trim().to_string();
    if token.is_empty() {
        return Err(
            "no HuggingFace token — run `niutero pdf-config <vault> --token-stdin` \
                    or add one in Settings → PDF attachments"
                .into(),
        );
    }
    let repo = reg
        .get(&v.root)
        .map(|r| r.pdf.repo.trim().to_string())
        .unwrap_or_default();
    if repo.is_empty() {
        return Err("no HF dataset repo configured for this vault — run \
                    `niutero pdf-config <vault> --repo user/repo` or set it in \
                    Settings → PDF attachments"
            .into());
    }
    Ok((token, repo))
}

/// **Online (HF).** Create the vault's dataset repo, private. Idempotent —
/// an already-existing repo reports success.
pub fn create_pdf_repo(v: &Vault) -> Result<String, String> {
    let (token, repo) = resolve_hf(v)?;
    niutero_online::hf_create_dataset(&token, &repo)
}

/// **Online (HF).** Upload an entry's local PDF to the vault's dataset repo
/// at `pdfs/<key>.pdf`. Returns the remote path.
pub fn pdf_push(v: &Vault, citekey: &str) -> Result<String, String> {
    let (token, repo) = resolve_hf(v)?;
    crate::entry_exists(v, citekey)?;
    let local = crate::pdf_path(v, citekey);
    if !local.exists() {
        return Err(format!(
            "no local PDF for '{citekey}' — attach or fetch one first"
        ));
    }
    let remote = format!("pdfs/{}.pdf", crate::pdf_stem(citekey));
    niutero_online::hf_upload(&token, &repo, &remote, &local)?;
    Ok(remote)
}

/// **Online (HF).** Download an entry's PDF from the vault's dataset repo
/// into `pdfs/`. A failed download leaves no partial file behind.
pub fn pdf_pull(v: &Vault, citekey: &str) -> Result<PathBuf, String> {
    let (token, repo) = resolve_hf(v)?;
    crate::entry_exists(v, citekey)?;
    let dest = crate::prepare_pdf_dir(v, citekey)?;
    let remote = format!("pdfs/{}.pdf", crate::pdf_stem(citekey));
    match niutero_online::hf_download(&token, &repo, &remote, &dest) {
        Ok(()) => Ok(dest),
        Err(e) => {
            let _ = std::fs::remove_file(&dest); // never leave a torn PDF
            Err(e)
        }
    }
}

/// A directly-fetchable PDF URL for an entry's `url`, if recognizable. Pure.
/// Direct `.pdf` links pass through; arXiv abs pages map to their PDF;
/// anything else (publisher landing pages) is `None` — auto-fetch must never
/// download an HTML page and call it a PDF.
pub fn fetchable_pdf_url(url: &str) -> Option<String> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return None;
    }
    let path = u.split(['?', '#']).next().unwrap_or(u);
    if path.to_ascii_lowercase().ends_with(".pdf") {
        return Some(u.to_string());
    }
    for prefix in [
        "https://arxiv.org/abs/",
        "http://arxiv.org/abs/",
        "https://www.arxiv.org/abs/",
    ] {
        if let Some(id) = path.strip_prefix(prefix) {
            let id = id.trim_matches('/');
            if !id.is_empty() {
                return Some(format!("https://arxiv.org/pdf/{id}"));
            }
        }
    }
    None
}

/// Post-import hook: fetch likely PDFs for `keys` when this vault opted in
/// (`pdf-config --auto-fetch true`). Best-effort per entry — a failed fetch
/// is skipped, never fatal, and leaves no partial file. Returns
/// `(fetched, attempted)`; `(0, 0)` without a single network call when the
/// pref is off, keeping the base import path fully offline.
pub fn auto_fetch_pdfs(v: &Vault, keys: &[String]) -> Result<(usize, usize), String> {
    let prefs = pdf_prefs(v)?;
    if !prefs.auto_fetch {
        return Ok((0, 0));
    }
    let mut fetched = 0usize;
    let mut attempted = 0usize;
    for k in keys {
        let Ok(view) = crate::show(v, k) else {
            continue;
        };
        if crate::pdf_path(v, k).exists() {
            continue; // never clobber an existing attachment
        }
        let Some(url) = view.fields.get("url").and_then(|u| fetchable_pdf_url(u)) else {
            continue;
        };
        attempted += 1;
        let dest = crate::prepare_pdf_dir(v, k)?;
        match niutero_online::fetch_to_file(&url, &dest) {
            Ok(()) => fetched += 1,
            Err(_) => {
                let _ = std::fs::remove_file(&dest);
            }
        }
    }
    Ok((fetched, attempted))
}

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

/// Legacy read of the machine-registry PDF prefs (the repo/auto-fetch briefly
/// lived there before moving into the vault's own `config.toml`). Kept as a
/// read-only fallback so existing setups don't lose their config.
pub(crate) fn pdf_prefs(v: &Vault) -> Result<PdfPrefs, String> {
    Ok(Registry::load()
        .map_err(|e| format!("read PDF prefs: {e}"))?
        .get(&v.root)
        .map(|r| r.pdf.clone())
        .unwrap_or_default())
}

/// Legacy write access to the registry copy (used only to clear it once the
/// value moves into the vault config).
pub(crate) fn update_pdf_prefs(
    v: &Vault,
    f: impl FnOnce(&mut PdfPrefs),
) -> Result<PdfPrefs, String> {
    let root = v.root.clone();
    with_registry_mut(|reg| {
        let rec = reg.entry_mut(&root);
        f(&mut rec.pdf);
        rec.pdf.clone()
    })
    .map_err(|e| format!("save PDF prefs: {e}"))
}

/// The library's HF dataset repo: `.niutero/config.toml` first (synced — the
/// repo is a library property collaborators share), with the legacy machine-
/// registry copy as a read fallback.
pub fn pdf_repo(v: &Vault) -> Result<Option<String>, String> {
    if let Some(r) = v
        .config
        .pdf_repo
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        return Ok(Some(r.to_string()));
    }
    let legacy = pdf_prefs(v)?.repo.trim().to_string();
    Ok((!legacy.is_empty()).then_some(legacy))
}

/// Set (or clear, with an empty string) the library's HF dataset repo in
/// `config.toml`. Validates the `user/repo` shape up front, and clears the
/// legacy registry copy so a cleared repo can't silently resurrect.
pub fn set_pdf_repo(v: &mut Vault, repo: &str) -> Result<(), String> {
    let repo = repo.trim();
    if !repo.is_empty() && !niutero_online::valid_hf_repo(repo) {
        return Err(format!(
            "'{repo}' isn't a valid HF dataset id (want user/repo)"
        ));
    }
    {
        let _lock = crate::lock_vault(v)?;
        v.config.pdf_repo = (!repo.is_empty()).then(|| repo.to_string());
        v.save_sidecar().map_err(|e| format!("save config: {e}"))?;
    }
    // Best-effort legacy cleanup — the vault config is the single source now.
    let _ = update_pdf_prefs(v, |p| p.repo = String::new());
    Ok(())
}

/// Whether post-import PDF auto-fetch is on for this library: the synced
/// `workflow.auto_fetch_pdf`, or the legacy machine-registry toggle.
pub fn pdf_auto_fetch_enabled(v: &Vault) -> bool {
    v.config.workflow.auto_fetch_pdf || pdf_prefs(v).map(|p| p.auto_fetch).unwrap_or(false)
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
    let Some(repo) = pdf_repo(v)? else {
        return Err("no HF dataset repo configured for this library — run \
                    `niutero pdf-config <vault> --repo user/repo` or set it in \
                    Settings → PDF attachments"
            .into());
    };
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
    if !pdf_auto_fetch_enabled(v) {
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

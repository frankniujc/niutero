//! Machine-local registry (`vaults.toml`).
//!
//! Unlike the `.niutero/` sidecar — which travels *with* the library and is
//! shared with collaborators — the registry is **per-machine** and never
//! committed/synced. It records the vaults this machine has opened (so a
//! GUI / shell can offer "recent libraries") plus the strictly machine-local
//! preferences that must NOT leak into the synced vault:
//!
//! * **keep-updated export targets** — absolute paths on *this* machine to
//!   re-export to on every change (e.g. an Overleaf checkout);
//! * **sync strategy** — whether `sync` pushes/pulls on *this* machine.
//!
//! It lives at a platform config path (or `$NIUTERO_REGISTRY` when set — used by
//! tests so they never touch the real machine file). The on-disk API is
//! *path-explicit* ([`Registry::load_from`] / [`Registry::save_to`]) so unit
//! tests run race-free without mutating any global env var.

// NEVER log Registry contents — it holds the AI key and the HF token.

use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The machine-local registry: a list of known vaults, most-recently-opened
/// first, plus machine-local LLM settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default, rename = "vault")]
    pub vaults: Vec<VaultRecord>,
    /// Machine-local LLM ("AI assistant") configuration. Stored here — never in
    /// a synced vault — so an API key can't leak into git.
    #[serde(default, skip_serializing_if = "AiConfig::is_default")]
    pub ai: AiConfig,
    /// HuggingFace access token for PDF sync (one per account, so it lives at
    /// the registry root, not per-vault). Machine-local for the same reason as
    /// the AI key: a secret must never ride the synced vault into git.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hf_token: String,
    /// This machine's GUI appearance (theme + accent). Personal — applies to
    /// every library on this machine and never syncs to collaborators.
    #[serde(default, skip_serializing_if = "UiPrefs::is_default")]
    pub ui: UiPrefs,
}

/// Machine-local GUI appearance prefs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UiPrefs {
    /// Dark mode (default light).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dark: bool,
    /// Accent swatch index (0 = the theme's own default green).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub accent: usize,
    /// Display author names as `First Last` instead of the BibTeX-conventional
    /// `Last, First` (display-only — the stored field is never rewritten).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub author_first_last: bool,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl UiPrefs {
    /// True when all-default, so the table is omitted from `vaults.toml`.
    pub fn is_default(&self) -> bool {
        *self == UiPrefs::default()
    }
}

/// Machine-local LLM settings (Settings → AI assistant). The API key lives here
/// in `vaults.toml`, never in a library or git.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AiConfig {
    /// Master switch — off by default; the AI features error until it's on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enabled: bool,
    /// Provider label (only "anthropic" is wired today; stored for the UI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    /// API key (falls back to `$ANTHROPIC_API_KEY` when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    /// Model id (falls back to a built-in default when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// Optional API base URL override.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
}

impl AiConfig {
    /// True when all-default, so it's omitted from `vaults.toml`.
    pub fn is_default(&self) -> bool {
        *self == AiConfig::default()
    }
}

/// One vault's machine-local record, keyed by its canonical path.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VaultRecord {
    /// Canonical path of the vault root (the registry key).
    pub path: PathBuf,
    /// Unix-epoch seconds of the last open, for recency ordering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<u64>,
    /// Keep-updated export targets re-written on every change (#45). Machine-
    /// local absolute paths — never synced into the vault.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<ExportTarget>,
    /// This machine's sync strategy for the vault (#48).
    #[serde(default, skip_serializing_if = "SyncPrefs::is_default")]
    pub sync: SyncPrefs,
    /// **Legacy** copy of the vault's PDF prefs (the HF repo / auto-fetch now
    /// live in the vault's synced `config.toml`). Read-only migration fallback,
    /// cleared by the engine on set — do not write new prefs here.
    #[serde(default, skip_serializing_if = "PdfPrefs::is_default")]
    pub pdf: PdfPrefs,
}

/// **Legacy** copy of one vault's PDF prefs: the HuggingFace **dataset** repo
/// and the auto-fetch toggle briefly lived here before moving into the vault's
/// own synced `config.toml`. Kept only as a read-only migration fallback — the
/// engine clears it whenever the value is set anew; do not write new prefs
/// here. The account token is [`Registry::hf_token`]. `auto_fetch` defaults
/// **off** — optional features must not put network calls on the base import
/// path uninvited.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfPrefs {
    /// HF dataset repo as `user/repo`; empty = HF sync not configured.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    /// After an import, fetch the new entries' PDFs when their url is a direct
    /// `.pdf` or an arXiv abs page (publisher landing pages are skipped).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_fetch: bool,
}

impl PdfPrefs {
    /// True when all-default, so the table is omitted from `vaults.toml`.
    pub fn is_default(&self) -> bool {
        *self == PdfPrefs::default()
    }
}

/// A keep-updated export target (#45): an external `.bib` re-written whenever the
/// library changes, optionally filtered by a saved query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportTarget {
    /// Absolute path of the `.bib` to (over)write on every change.
    pub out: PathBuf,
    /// Optional filter query; `None` exports the whole library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Machine-local sync strategy (#48). Push/pull timing is a per-machine choice
/// (a laptop may sync both ways; a shared box might only pull), so it lives here
/// rather than in the synced sidecar. Defaults to full two-way sync.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncPrefs {
    /// Pull (and merge) from `origin` before pushing. Default `true`.
    #[serde(default = "yes")]
    pub pull: bool,
    /// Push to `origin` after committing/merging. Default `true`.
    #[serde(default = "yes")]
    pub push: bool,
}

fn yes() -> bool {
    true
}

impl Default for SyncPrefs {
    fn default() -> Self {
        Self {
            pull: true,
            push: true,
        }
    }
}

impl SyncPrefs {
    /// True when these are the defaults, so they're omitted on disk.
    pub fn is_default(&self) -> bool {
        *self == SyncPrefs::default()
    }
}

impl Registry {
    /// Load the registry from an explicit path. A missing file is an empty
    /// registry (not an error); a present-but-corrupt file errors rather than
    /// silently discarding the machine's recorded vaults.
    pub fn load_from(path: &Path) -> io::Result<Registry> {
        match fs::read_to_string(path) {
            Ok(s) if s.trim().is_empty() => Ok(Registry::default()),
            Ok(s) => toml::from_str(&s)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(e),
        }
    }

    /// Write the registry to an explicit path, creating the parent dir. The
    /// write is atomic (temp + rename) and owner-only on Unix — the registry
    /// can hold the AI API key, so it must not be world-readable at rest.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let body = toml::to_string(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        crate::atomic_write_private(path, &body)
    }

    /// Load from the machine's default registry path (or `$NIUTERO_REGISTRY`).
    pub fn load() -> io::Result<Registry> {
        Registry::load_from(&registry_path()?)
    }

    /// Save to the machine's default registry path (or `$NIUTERO_REGISTRY`).
    pub fn save(&self) -> io::Result<()> {
        self.save_to(&registry_path()?)
    }

    /// The record for `path` (canonicalized), if known.
    pub fn get(&self, path: &Path) -> Option<&VaultRecord> {
        let key = normalize_path(path);
        self.vaults.iter().find(|r| r.path == key)
    }

    /// The record for `path`, inserting an empty one (at the front) if absent,
    /// so callers can mutate prefs in place.
    pub fn entry_mut(&mut self, path: &Path) -> &mut VaultRecord {
        let key = normalize_path(path);
        if let Some(i) = self.vaults.iter().position(|r| r.path == key) {
            return &mut self.vaults[i];
        }
        self.vaults.insert(
            0,
            VaultRecord {
                path: key,
                ..Default::default()
            },
        );
        &mut self.vaults[0]
    }

    /// Record that `path` was just opened: stamp `last_opened` with `now` and
    /// move it to the front (most-recent-first ordering).
    pub fn record_open(&mut self, path: &Path, now: u64) {
        let key = normalize_path(path);
        let mut rec = match self.vaults.iter().position(|r| r.path == key) {
            Some(i) => self.vaults.remove(i),
            None => VaultRecord {
                path: key,
                ..Default::default()
            },
        };
        rec.last_opened = Some(now);
        self.vaults.insert(0, rec);
    }

    /// Drop the record for `path`. Returns whether one was present.
    pub fn forget(&mut self, path: &Path) -> bool {
        let key = normalize_path(path);
        let before = self.vaults.len();
        self.vaults.retain(|r| r.path != key);
        self.vaults.len() != before
    }
}

/// Resolve the default registry file path: `$NIUTERO_REGISTRY` if set (tests use
/// this), else the platform per-user config dir + `niutero/vaults.toml`. No new
/// dependency — resolved from the standard env vars directly.
pub fn registry_path() -> io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("NIUTERO_REGISTRY") {
        // A leaked test env var is maddening to spot otherwise.
        let p = PathBuf::from(p);
        log::debug!("registry: using $NIUTERO_REGISTRY override {}", p.display());
        return Ok(p);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| {
        let p = b.join("niutero").join("vaults.toml");
        log::debug!("registry: using platform path {}", p.display());
        p
    })
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine a config directory for the niutero registry",
        )
    })
}

/// Load the default registry, run `f` against it, and save — all while holding
/// an exclusive cross-process lock on the registry file. Because `vaults.toml`
/// is one machine-global file mutated by an unsynchronized read-modify-write,
/// concurrent `niutero` processes (e.g. a `list` recording its open while an
/// `export-target add` is mid-save) would otherwise clobber each other's
/// changes. The lock serializes the whole cycle so a confirmed change is never
/// lost. The lock file is a sibling `*.lock` (machine-local, never synced).
pub fn with_registry_mut<T>(f: impl FnOnce(&mut Registry) -> T) -> io::Result<T> {
    let path = registry_path()?;
    let _lock = RegistryLock::acquire(&path)?;
    let mut reg = Registry::load_from(&path)?;
    let out = f(&mut reg);
    reg.save_to(&path)?;
    Ok(out)
}

/// An exclusive advisory lock on the registry file, held for one
/// load-modify-save cycle (see [`with_registry_mut`]).
struct RegistryLock {
    _file: File,
}

impl RegistryLock {
    fn acquire(registry_path: &Path) -> io::Result<RegistryLock> {
        if let Some(parent) = registry_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_sibling(registry_path))?;
        // Blocking exclusive lock: registry ops are tiny, so a brief wait is
        // preferable to a spurious failure. Released when the guard drops; the
        // OS releases it on process exit too, so a crash can't wedge it.
        file.lock()?;
        Ok(RegistryLock { _file: file })
    }
}

/// `<registry>.lock` — the lock file lives beside the registry it guards.
fn lock_sibling(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

/// Seconds since the Unix epoch, for `last_opened` stamps. Saturates to 0 if the
/// clock is before 1970 (never panics).
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Canonicalize a vault path to a stable registry key. Falls back to the path
/// as-given if it doesn't exist yet, and strips Windows' `\\?\` extended-length
/// prefix so stored/displayed paths stay readable.
fn normalize_path(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    strip_extended_prefix(canonical)
}

fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// One shared lock for every test that mutates the process-global
    /// `$NIUTERO_REGISTRY`, so they never race each other.
    fn env_lock() -> MutexGuard<'static, ()> {
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn missing_file_is_an_empty_registry() {
        let dir = tmp();
        let reg = Registry::load_from(&dir.path().join("vaults.toml")).unwrap();
        assert!(reg.vaults.is_empty());
    }

    #[test]
    fn record_open_orders_most_recent_first_and_dedupes() {
        let dir = tmp();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let mut reg = Registry::default();
        reg.record_open(&a, 100);
        reg.record_open(&b, 200);
        // Re-opening `a` moves it back to the front and updates the timestamp —
        // without creating a duplicate record.
        reg.record_open(&a, 300);
        assert_eq!(reg.vaults.len(), 2);
        assert_eq!(reg.vaults[0].path, normalize_path(&a));
        assert_eq!(reg.vaults[0].last_opened, Some(300));
        assert_eq!(reg.vaults[1].path, normalize_path(&b));
    }

    #[test]
    fn roundtrips_through_disk_with_prefs() {
        let dir = tmp();
        let vault = dir.path().join("lib");
        fs::create_dir_all(&vault).unwrap();
        let path = dir.path().join("vaults.toml");

        let mut reg = Registry::default();
        reg.record_open(&vault, 42);
        let rec = reg.entry_mut(&vault);
        rec.exports.push(ExportTarget {
            out: PathBuf::from("/tmp/overleaf/refs.bib"),
            query: Some("tag:thesis".into()),
        });
        rec.sync = SyncPrefs {
            pull: true,
            push: false,
        };
        reg.save_to(&path).unwrap();

        let back = Registry::load_from(&path).unwrap();
        assert_eq!(back, reg);
        let r = back.get(&vault).unwrap();
        assert_eq!(r.exports.len(), 1);
        assert_eq!(r.exports[0].query.as_deref(), Some("tag:thesis"));
        assert!(!r.sync.push);
    }

    #[test]
    fn default_sync_prefs_are_omitted_on_disk() {
        let dir = tmp();
        let vault = dir.path().join("lib");
        fs::create_dir_all(&vault).unwrap();
        let path = dir.path().join("vaults.toml");

        let mut reg = Registry::default();
        reg.record_open(&vault, 1);
        reg.save_to(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        // Two-way sync is the default, so no [vault.sync] table is written.
        assert!(!text.contains("sync"), "default sync prefs leaked: {text}");
        // And an absent table reads back as the default (pull && push).
        let back = Registry::load_from(&path).unwrap();
        assert!(back.get(&vault).unwrap().sync.is_default());
    }

    /// Unix-only (the dev machine is Windows; CI's Linux leg runs this): a
    /// registry that can hold an API key must land owner-only at rest.
    #[cfg(unix)]
    #[test]
    fn registry_file_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp();
        let path = dir.path().join("vaults.toml");
        let mut reg = Registry::default();
        reg.ai.enabled = true;
        reg.ai.api_key = "sk-secret".into();
        reg.save_to(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "registry must be owner-only, got {mode:o}");
    }

    #[test]
    fn ui_prefs_roundtrip_and_defaults_are_omitted() {
        let dir = tmp();
        let path = dir.path().join("vaults.toml");
        let mut reg = Registry::default();
        reg.save_to(&path).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("[ui]"));
        reg.ui = UiPrefs {
            dark: true,
            accent: 2,
            ..Default::default()
        };
        reg.save_to(&path).unwrap();
        let back = Registry::load_from(&path).unwrap();
        assert!(back.ui.dark);
        assert_eq!(back.ui.accent, 2);
    }

    #[test]
    fn forget_removes_the_record() {
        let dir = tmp();
        let vault = dir.path().join("lib");
        fs::create_dir_all(&vault).unwrap();
        let mut reg = Registry::default();
        reg.record_open(&vault, 1);
        assert!(reg.forget(&vault));
        assert!(reg.vaults.is_empty());
        // Forgetting an unknown vault is a no-op, not an error.
        assert!(!reg.forget(&vault));
    }

    #[test]
    fn registry_path_prefers_the_env_override() {
        let _g = env_lock();
        std::env::set_var("NIUTERO_REGISTRY", "/custom/vaults.toml");
        assert_eq!(
            registry_path().unwrap(),
            PathBuf::from("/custom/vaults.toml")
        );
        std::env::remove_var("NIUTERO_REGISTRY");
    }

    #[test]
    fn with_registry_mut_persists_under_the_lock() {
        let _g = env_lock();
        let dir = tmp();
        let regpath = dir.path().join("cfg").join("vaults.toml");
        std::env::set_var("NIUTERO_REGISTRY", &regpath);
        let vault = dir.path().join("lib");
        fs::create_dir_all(&vault).unwrap();

        // A locked read-modify-save round-trips, creating the config dir.
        with_registry_mut(|reg| reg.record_open(&vault, 7)).unwrap();
        with_registry_mut(|reg| {
            reg.entry_mut(&vault).exports.push(ExportTarget {
                out: PathBuf::from("/tmp/m.bib"),
                query: None,
            })
        })
        .unwrap();

        let back = Registry::load_from(&regpath).unwrap();
        assert_eq!(back.vaults.len(), 1);
        assert_eq!(back.vaults[0].last_opened, Some(7));
        assert_eq!(back.vaults[0].exports.len(), 1);

        std::env::remove_var("NIUTERO_REGISTRY");
    }
}

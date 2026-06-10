//! niutero-vault — vault IO.
//!
//! A library is a folder: a portable `references.bib` (the source of truth)
//! plus a hidden `.niutero/` sidecar holding niutero's private data:
//!
//! ```text
//! <vault>/
//! ├── references.bib   # niutero-agnostic — never carries private data
//! └── .niutero/
//!     ├── config.toml  # library name + schema version
//!     ├── meta.json    # per-citekey tags / notes / added time
//!     └── views.toml   # named saved filter views
//! ```
//!
//! The `.bib` is parsed/serialized via `niutero-bib`; the sidecar is the only
//! place niutero's own data lives, so a collaborator who doesn't use niutero
//! still gets a clean `references.bib`.

use std::collections::BTreeMap;
use std::fs;
use std::fs::{File, OpenOptions, TryLockError};
use std::hash::{Hash, Hasher};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use niutero_bib::{parse, to_bibtex, BibItem};
use serde::{Deserialize, Serialize};

pub mod registry;
pub use registry::{AiConfig, ExportTarget, PdfPrefs, Registry, SyncPrefs, UiPrefs, VaultRecord};

pub const SCHEMA_VERSION: u32 = 1;

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

/// `.niutero/config.toml` — library-level settings. Everything here travels
/// WITH the library (synced via git), so it holds the library's properties —
/// never machine-personal prefs (those live in the registry) and never
/// secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// Citation-key pattern for generated keys (e.g. `{auth}{year}{Title.2}`).
    /// `None` means the engine's built-in default applies. Synced with the
    /// library, so collaborators share one key convention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citekey_pattern: Option<String>,
    /// HuggingFace **dataset** repo (`user/repo`) this library's PDFs push/pull
    /// to. Library property (collaborators share the repo); the account token
    /// stays machine-local in the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_repo: Option<String>,
    /// Shared workflow behavior for this library (all default-off so the base
    /// path stays offline and nothing commits uninvited).
    #[serde(default, skip_serializing_if = "WorkflowConfig::is_default")]
    pub workflow: WorkflowConfig,
}

/// Library-wide workflow toggles (Settings → Workflow). Synced with the
/// library so collaborators share one policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// After an import, fill missing fields from each new entry's DOI.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enrich_on_import: bool,
    /// After every library mutation, `git commit` the vault (no push).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_commit: bool,
    /// Default duplicate policy for imports when none is given explicitly:
    /// `skip` / `overwrite` / `rename`. `None` = the tool's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_dup: Option<String>,
    /// After an import, fetch new entries' PDFs when their url is a direct
    /// `.pdf` or an arXiv abs page.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_fetch_pdf: bool,
}

impl WorkflowConfig {
    /// True when all-default, so the table is omitted from `config.toml`.
    pub fn is_default(&self) -> bool {
        *self == WorkflowConfig::default()
    }
}

impl Config {
    fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: SCHEMA_VERSION,
            citekey_pattern: None,
            pdf_repo: None,
            workflow: WorkflowConfig::default(),
        }
    }
}

/// A reading-workflow state for an entry. `Unread` is the default and is never
/// persisted (an absent `status` *is* unread), so `meta.json` stays minimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Unread,
    Reading,
    Done,
}

impl Status {
    /// The lowercase name, as written in `meta.json` and matched by queries.
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Unread => "unread",
            Status::Reading => "reading",
            Status::Done => "done",
        }
    }
}

/// Per-citekey private data. Empty fields are omitted on disk so `meta.json`
/// stays small and diffs stay clean.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMeta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Reading workflow state. `None`/absent == unread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// Star rating, 1–5. `None`/absent == unrated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stars: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<String>,
}

impl EntryMeta {
    /// True when there is nothing worth persisting, so callers can drop the
    /// map entry and keep `meta.json` minimal.
    pub fn is_empty(&self) -> bool {
        // Fold the defaults: an explicit `unread` / `0` (e.g. from a hand-edited
        // meta.json) counts as nothing, so the entry is still pruned to keep the
        // sidecar minimal.
        self.tags.is_empty()
            && self.note.is_empty()
            && self.added.is_none()
            && self.status.is_none_or(|s| s == Status::Unread)
            && self.stars.is_none_or(|n| n == 0)
    }
}

/// `.niutero/meta.json` — keyed by cite key, ordered for stable diffs.
pub type Meta = BTreeMap<String, EntryMeta>;

/// One named saved filter (a "collection" is just a saved query).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    pub query: String,
}

/// `.niutero/views.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Views {
    #[serde(default)]
    pub views: Vec<View>,
}

/// An open vault: its root path plus the loaded sidecar.
#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub config: Config,
    pub meta: Meta,
    pub views: Views,
}

impl Vault {
    pub fn bib_path(&self) -> PathBuf {
        self.root.join("references.bib")
    }
    pub fn niutero_dir(&self) -> PathBuf {
        self.root.join(".niutero")
    }
    fn config_path(&self) -> PathBuf {
        self.niutero_dir().join("config.toml")
    }
    fn meta_path(&self) -> PathBuf {
        self.niutero_dir().join("meta.json")
    }
    fn views_path(&self) -> PathBuf {
        self.niutero_dir().join("views.toml")
    }

    /// Create the vault layout under `root`: the folder, a default `.niutero/`
    /// sidecar, and an empty `references.bib` *only if absent* (never clobber
    /// the source of truth). Errors if `root` is already a vault.
    pub fn init(root: impl AsRef<Path>) -> io::Result<Vault> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        if root.join(".niutero").join("config.toml").exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is already a niutero vault", root.display()),
            ));
        }
        let vault = Vault {
            config: Config::named(folder_name(&root)),
            meta: Meta::new(),
            views: Views::default(),
            root,
        };
        if !vault.bib_path().exists() {
            atomic_write(&vault.bib_path(), "")?;
        }
        vault.save_sidecar()?;
        Ok(vault)
    }

    /// Open a folder as a vault, loading the sidecar if present. A plain folder
    /// without `.niutero/` opens with in-memory defaults (nothing is written),
    /// so read-only commands work on it.
    pub fn open(root: impl AsRef<Path>) -> io::Result<Vault> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a directory", root.display()),
            ));
        }
        // Fall back to defaults only when a sidecar file is *absent*. Other IO
        // errors (permissions, a half-written file) propagate, so we never mask
        // a corrupt sidecar and then overwrite it with empty defaults.
        let config = match fs::read_to_string(root.join(".niutero").join("config.toml")) {
            Ok(s) => toml::from_str(&s).map_err(invalid_data)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Config::named(folder_name(&root)),
            Err(e) => return Err(e),
        };
        let meta = match fs::read_to_string(root.join(".niutero").join("meta.json")) {
            Ok(s) if s.trim().is_empty() => Meta::new(),
            Ok(s) => serde_json::from_str(&s).map_err(invalid_data)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Meta::new(),
            Err(e) => return Err(e),
        };
        let views = match fs::read_to_string(root.join(".niutero").join("views.toml")) {
            Ok(s) => toml::from_str(&s).map_err(invalid_data)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Views::default(),
            Err(e) => return Err(e),
        };
        Ok(Vault {
            root,
            config,
            meta,
            views,
        })
    }

    /// Write the whole sidecar to disk (creating `.niutero/` if needed). Each
    /// file is written atomically; the three are not transactional together,
    /// but each is individually crash-consistent.
    pub fn save_sidecar(&self) -> io::Result<()> {
        fs::create_dir_all(self.niutero_dir())?;
        atomic_write(
            &self.config_path(),
            &toml::to_string(&self.config).map_err(invalid_data)?,
        )?;
        let mut json = serde_json::to_string_pretty(&self.meta).map_err(invalid_data)?;
        json.push('\n');
        atomic_write(&self.meta_path(), &json)?;
        atomic_write(
            &self.views_path(),
            &toml::to_string(&self.views).map_err(invalid_data)?,
        )?;
        Ok(())
    }

    /// Parse `references.bib` into an item stream (empty if the file is absent).
    pub fn read_items(&self) -> io::Result<Vec<BibItem>> {
        match fs::read_to_string(self.bib_path()) {
            Ok(s) => Ok(parse(&s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Serialize an item stream back to `references.bib` deterministically and
    /// atomically (the source of truth is never left half-written).
    pub fn write_items(&self, items: &[BibItem]) -> io::Result<()> {
        atomic_write(&self.bib_path(), &to_bibtex(items))
    }

    /// Take an exclusive lock for the duration of a mutating operation, so two
    /// `niutero` processes can't interleave read-modify-write and lose an
    /// update. Errors with [`io::ErrorKind::WouldBlock`] if another process
    /// already holds it. The lock file lives in the system temp dir (keyed by
    /// the vault path), so it never enters the vault or git. Released when the
    /// returned guard is dropped.
    pub fn lock(&self) -> io::Result<VaultLock> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path(&self.root))?;
        match file.try_lock() {
            Ok(()) => Ok(VaultLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "another niutero process is modifying this library",
            )),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }
}

/// An exclusive advisory lock on a vault (see [`Vault::lock`]). The lock is held
/// until this guard is dropped.
#[derive(Debug)]
pub struct VaultLock {
    _file: File,
}

/// A stable, machine-local lock-file path for a vault, in the temp dir so it is
/// never committed/synced. Keyed by the canonicalized vault path.
fn lock_path(root: &Path) -> PathBuf {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    std::env::temp_dir().join(format!("niutero-{:016x}.lock", hasher.finish()))
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `contents` to `path` atomically: a uniquely-named temp file in the
/// same directory is written and fsynced, then renamed over `path`. `rename`
/// is atomic on the same volume and replaces an existing file (including on
/// Windows), so a crash leaves either the old file or the new one intact —
/// never a truncated `references.bib`.
fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    atomic_write_impl(path, contents, false)
}

/// Like [`atomic_write`], but the file ends up owner-only (0600) on Unix —
/// for machine-local files that can hold secrets, like the registry once an
/// AI key is stored in it. (`rename` keeps the temp file's mode, so the
/// restriction survives the swap.) On Windows the per-user `%APPDATA%`
/// default ACL already scopes the file to the user.
pub(crate) fn atomic_write_private(path: &Path, contents: &str) -> io::Result<()> {
    atomic_write_impl(path, contents, true)
}

fn atomic_write_impl(path: &Path, contents: &str, private: bool) -> io::Result<()> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let stem = path.file_name().and_then(|s| s.to_str()).unwrap_or("out");
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{stem}.{}.{n}.tmp", std::process::id()));

    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        let _ = private;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp); // best-effort cleanup
    }
    result
}

/// Atomically write `contents` to `path` (temp + fsync + rename), the same
/// crash-safe primitive the vault uses for `references.bib`. Exposed so the
/// engine can give keep-updated export mirrors the same guarantee.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    atomic_write(path, contents)
}

fn folder_name(root: &Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("library")
        .to_string()
}

fn invalid_data<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use niutero_bib::{entries, BibItem};
    use niutero_core::BibEntry;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn init_creates_layout() {
        let dir = tmp();
        let root = dir.path().join("MyLib");
        let v = Vault::init(&root).unwrap();
        assert!(root.join("references.bib").exists());
        assert!(root.join(".niutero").join("config.toml").exists());
        assert!(root.join(".niutero").join("meta.json").exists());
        assert!(root.join(".niutero").join("views.toml").exists());
        assert_eq!(v.config.name, "MyLib");
        assert_eq!(v.config.schema, SCHEMA_VERSION);
    }

    #[test]
    fn init_twice_errors() {
        let dir = tmp();
        Vault::init(dir.path()).unwrap();
        let err = Vault::init(dir.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn init_does_not_clobber_existing_bib() {
        let dir = tmp();
        fs::write(dir.path().join("references.bib"), "@misc{keep}\n").unwrap();
        Vault::init(dir.path()).unwrap();
        let s = fs::read_to_string(dir.path().join("references.bib")).unwrap();
        assert!(s.contains("@misc{keep}"));
    }

    #[test]
    fn sidecar_roundtrips() {
        let dir = tmp();
        let mut v = Vault::init(dir.path()).unwrap();
        v.meta.insert(
            "niu2025".to_string(),
            EntryMeta {
                tags: vec!["nlp".into(), "llm".into()],
                note: "key paper".into(),
                status: Some(Status::Reading),
                stars: Some(4),
                added: Some("2026-05-28".into()),
            },
        );
        v.views.views.push(View {
            name: "NLP".into(),
            query: "tag:nlp".into(),
        });
        v.save_sidecar().unwrap();

        let reopened = Vault::open(dir.path()).unwrap();
        assert_eq!(reopened.meta, v.meta);
        assert_eq!(reopened.views, v.views);
        assert_eq!(reopened.config, v.config);
    }

    #[test]
    fn bib_items_roundtrip_through_vault() {
        let dir = tmp();
        let v = Vault::init(dir.path()).unwrap();
        let items = vec![BibItem::Entry(
            BibEntry::new("article", "k").with_field("title", "Hi"),
        )];
        v.write_items(&items).unwrap();
        let read = v.read_items().unwrap();
        assert_eq!(entries(&read).count(), 1);
        assert_eq!(read, items);
    }

    #[test]
    fn open_plain_folder_uses_defaults() {
        let dir = tmp();
        // no .niutero/, but a references.bib exists
        fs::write(dir.path().join("references.bib"), "@misc{k}\n").unwrap();
        let v = Vault::open(dir.path()).unwrap();
        assert!(v.meta.is_empty());
        assert_eq!(entries(&v.read_items().unwrap()).count(), 1);
        // nothing was written
        assert!(!dir.path().join(".niutero").exists());
    }

    #[test]
    fn config_roundtrips_pdf_repo_and_workflow() {
        let dir = tmp();
        let mut v = Vault::init(dir.path()).unwrap();
        // Defaults are omitted on disk entirely.
        let text = fs::read_to_string(v.config_path()).unwrap();
        assert!(
            !text.contains("workflow") && !text.contains("pdf_repo"),
            "{text}"
        );

        v.config.pdf_repo = Some("frank/papers".into());
        v.config.workflow.enrich_on_import = true;
        v.config.workflow.on_dup = Some("overwrite".into());
        v.save_sidecar().unwrap();
        let back = Vault::open(dir.path()).unwrap();
        assert_eq!(back.config.pdf_repo.as_deref(), Some("frank/papers"));
        assert!(back.config.workflow.enrich_on_import);
        assert!(!back.config.workflow.auto_commit);
        assert_eq!(back.config.workflow.on_dup.as_deref(), Some("overwrite"));
        // An old config.toml without the new tables still opens (serde defaults).
        fs::write(v.config_path(), "name = \"old\"\nschema = 1\n").unwrap();
        let old = Vault::open(dir.path()).unwrap();
        assert!(old.config.workflow.is_default() && old.config.pdf_repo.is_none());
    }

    #[test]
    fn save_sidecar_is_stable() {
        let dir = tmp();
        let v = Vault::init(dir.path()).unwrap();
        let first = fs::read_to_string(v.meta_path()).unwrap();
        v.save_sidecar().unwrap();
        let second = fs::read_to_string(v.meta_path()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn atomic_writes_leave_no_temp_files() {
        let dir = tmp();
        let v = Vault::init(dir.path()).unwrap();
        v.write_items(&[BibItem::Entry(BibEntry::new("misc", "k"))])
            .unwrap();
        v.save_sidecar().unwrap();
        let tmp_count = |p: &std::path::Path| {
            fs::read_dir(p)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                .count()
        };
        assert_eq!(tmp_count(dir.path()), 0, "no temp files left in vault root");
        assert_eq!(
            tmp_count(&v.niutero_dir()),
            0,
            "no temp files left in .niutero"
        );
        assert_eq!(fs::read_to_string(v.bib_path()).unwrap(), "@misc{k\n}\n");
    }

    #[test]
    fn write_items_overwrites_atomically() {
        let dir = tmp();
        let v = Vault::init(dir.path()).unwrap();
        v.write_items(&[BibItem::Entry(BibEntry::new("misc", "a"))])
            .unwrap();
        v.write_items(&[BibItem::Entry(BibEntry::new("misc", "b"))])
            .unwrap();
        let s = fs::read_to_string(v.bib_path()).unwrap();
        assert!(s.contains("@misc{b"));
        assert!(!s.contains("@misc{a"));
    }

    #[test]
    fn exclusive_lock_blocks_a_second_holder() {
        let dir = tmp();
        let v = Vault::init(dir.path()).unwrap();
        let guard = v.lock().expect("first lock");
        // A second acquisition (even in-process, via a separate handle) is denied.
        assert_eq!(v.lock().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        drop(guard);
        // Once released, it can be re-taken.
        assert!(v.lock().is_ok());
    }

    #[test]
    fn open_errors_on_malformed_sidecar() {
        // A present-but-corrupt sidecar must error, not silently fall back to
        // defaults (which a later save would then persist over the real data).
        let dir = tmp();
        Vault::init(dir.path()).unwrap();
        fs::write(
            dir.path().join(".niutero").join("config.toml"),
            "not valid = = toml {{{",
        )
        .unwrap();
        assert!(Vault::open(dir.path()).is_err());
    }
}

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
use std::io;
use std::path::{Path, PathBuf};

use niutero_bib::{parse, to_bibtex, BibItem};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

/// `.niutero/config.toml` — library-level settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    #[serde(default = "default_schema")]
    pub schema: u32,
}

impl Config {
    fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: SCHEMA_VERSION,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<String>,
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
            fs::write(vault.bib_path(), "")?;
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
        let config = match fs::read_to_string(root.join(".niutero").join("config.toml")) {
            Ok(s) => toml::from_str(&s).map_err(invalid_data)?,
            Err(_) => Config::named(folder_name(&root)),
        };
        let meta = match fs::read_to_string(root.join(".niutero").join("meta.json")) {
            Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s).map_err(invalid_data)?,
            _ => Meta::new(),
        };
        let views = match fs::read_to_string(root.join(".niutero").join("views.toml")) {
            Ok(s) => toml::from_str(&s).map_err(invalid_data)?,
            Err(_) => Views::default(),
        };
        Ok(Vault {
            root,
            config,
            meta,
            views,
        })
    }

    /// Write the whole sidecar to disk (creating `.niutero/` if needed).
    pub fn save_sidecar(&self) -> io::Result<()> {
        fs::create_dir_all(self.niutero_dir())?;
        fs::write(
            self.config_path(),
            toml::to_string(&self.config).map_err(invalid_data)?,
        )?;
        let mut json = serde_json::to_string_pretty(&self.meta).map_err(invalid_data)?;
        json.push('\n');
        fs::write(self.meta_path(), json)?;
        fs::write(
            self.views_path(),
            toml::to_string(&self.views).map_err(invalid_data)?,
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

    /// Serialize an item stream back to `references.bib` deterministically.
    pub fn write_items(&self, items: &[BibItem]) -> io::Result<()> {
        fs::write(self.bib_path(), to_bibtex(items))
    }
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
    fn save_sidecar_is_stable() {
        let dir = tmp();
        let v = Vault::init(dir.path()).unwrap();
        let first = fs::read_to_string(v.meta_path()).unwrap();
        v.save_sidecar().unwrap();
        let second = fs::read_to_string(v.meta_path()).unwrap();
        assert_eq!(first, second);
    }
}

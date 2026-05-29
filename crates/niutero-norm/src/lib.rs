//! niutero-norm — offline, propose-only normalization rules.
//!
//! Pure transformations on a [`BibEntry`]: drop configured noise fields, tidy
//! whitespace in values, and give arXiv entries a consistent `archiveprefix`.
//! Every rule is idempotent, so re-normalizing a normalized entry is a no-op.
//! No network — online enrichment (Semantic Scholar / DBLP / …) is a separate
//! concern. Callers show the returned change notes and only write on the user's
//! say-so; this crate never touches disk except to *read* an optional config.

use std::path::Path;

use niutero_core::BibEntry;
use serde::{Deserialize, Serialize};

/// Per-vault normalization settings (`.niutero/norm.toml`). Missing keys fall
/// back to the defaults, so a partial file is fine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NormConfig {
    /// Field names to remove (case-insensitive).
    pub drop_fields: Vec<String>,
    /// Collapse internal whitespace runs and trim each field value.
    pub tidy_whitespace: bool,
    /// Add `archiveprefix = {arXiv}` to entries that have an `eprint`.
    pub arxiv: bool,
}

impl Default for NormConfig {
    fn default() -> Self {
        Self {
            drop_fields: ["abstract", "file", "keywords", "urldate", "annote"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            tidy_whitespace: true,
            arxiv: true,
        }
    }
}

impl NormConfig {
    /// Load `<niutero_dir>/norm.toml`, falling back to defaults if it is absent
    /// or unreadable/unparseable (tolerant — normalization should never fail to
    /// start over a config typo).
    pub fn load(niutero_dir: &Path) -> Self {
        match std::fs::read_to_string(niutero_dir.join("norm.toml")) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Write a documented default `norm.toml` into `<niutero_dir>`, only if one
    /// isn't already there (so `init` can surface the knobs).
    pub fn write_default_if_absent(niutero_dir: &Path) -> std::io::Result<()> {
        let path = niutero_dir.join("norm.toml");
        if path.exists() {
            return Ok(());
        }
        std::fs::write(path, DEFAULT_NORM_TOML)
    }
}

/// The documented default written by [`NormConfig::write_default_if_absent`].
/// Kept in sync with [`NormConfig::default`] (a test asserts they match).
const DEFAULT_NORM_TOML: &str = "\
# niutero offline normalization config. `niutero normalize` is propose-only:
# it shows what would change; nothing is written without --write.

# Field names to drop from entries.
drop_fields = [\"abstract\", \"file\", \"keywords\", \"urldate\", \"annote\"]

# Collapse runs of whitespace and trim each field value.
tidy_whitespace = true

# Add `archiveprefix = {arXiv}` to entries that have an `eprint`.
arxiv = true
";

/// Apply the offline rules to `entry`, returning the normalized entry and a
/// list of human-readable notes describing what changed (empty if nothing did).
pub fn normalize_entry(entry: &BibEntry, cfg: &NormConfig) -> (BibEntry, Vec<String>) {
    let mut out = entry.clone();
    let mut notes = Vec::new();

    // 1. Drop noise fields.
    for f in &cfg.drop_fields {
        if out.remove(f).is_some() {
            notes.push(format!("dropped '{}'", f.to_ascii_lowercase()));
        }
    }

    // 2. Collapse whitespace runs and trim each value.
    if cfg.tidy_whitespace {
        let names: Vec<String> = out.fields.keys().cloned().collect();
        for name in names {
            let value = out.get(&name).unwrap_or("").to_string();
            let tidied = value.split_whitespace().collect::<Vec<_>>().join(" ");
            if tidied != value {
                out.set(&name, tidied);
                notes.push(format!("tidied whitespace in '{name}'"));
            }
        }
    }

    // 3. arXiv: a consistent archive prefix when there's an eprint id.
    if cfg.arxiv && out.get("eprint").is_some() && out.get("archiveprefix").is_none() {
        out.set("archiveprefix", "arXiv");
        notes.push("added archiveprefix = {arXiv}".to_string());
    }

    (out, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> BibEntry {
        BibEntry::new("article", "k")
            .with_field("title", "A   Theory\n  of  Stuff")
            .with_field("abstract", "long text")
            .with_field("year", "2020")
    }

    #[test]
    fn drops_and_tidies() {
        let (out, notes) = normalize_entry(&entry(), &NormConfig::default());
        assert_eq!(out.get("abstract"), None);
        assert_eq!(out.get("title"), Some("A Theory of Stuff"));
        assert_eq!(out.get("year"), Some("2020")); // untouched
        assert!(notes.iter().any(|n| n.contains("dropped 'abstract'")));
        assert!(notes
            .iter()
            .any(|n| n.contains("tidied whitespace in 'title'")));
    }

    #[test]
    fn is_idempotent() {
        let cfg = NormConfig::default();
        let (once, _) = normalize_entry(&entry(), &cfg);
        let (twice, notes) = normalize_entry(&once, &cfg);
        assert_eq!(once, twice);
        assert!(
            notes.is_empty(),
            "second pass should change nothing: {notes:?}"
        );
    }

    #[test]
    fn arxiv_adds_archiveprefix_once() {
        let cfg = NormConfig::default();
        let e = BibEntry::new("misc", "k").with_field("eprint", "2501.00001");
        let (out, notes) = normalize_entry(&e, &cfg);
        assert_eq!(out.get("archiveprefix"), Some("arXiv"));
        assert!(notes.iter().any(|n| n.contains("archiveprefix")));
        // already has it -> no further change
        let (_, notes2) = normalize_entry(&out, &cfg);
        assert!(notes2.is_empty());
    }

    #[test]
    fn respects_config() {
        let cfg = NormConfig {
            drop_fields: vec!["year".to_string()],
            tidy_whitespace: false,
            arxiv: false,
        };
        let (out, _) = normalize_entry(&entry(), &cfg);
        assert_eq!(out.get("year"), None); // dropped per config
        assert_eq!(out.get("abstract"), Some("long text")); // not in drop list now
        assert_eq!(out.get("title"), Some("A   Theory\n  of  Stuff")); // tidy off
    }

    #[test]
    fn clean_entry_yields_no_notes() {
        let e = BibEntry::new("article", "k").with_field("title", "Clean Title");
        let (_, notes) = normalize_entry(&e, &NormConfig::default());
        assert!(notes.is_empty());
    }

    #[test]
    fn default_toml_matches_default_config() {
        let parsed: NormConfig = toml::from_str(DEFAULT_NORM_TOML).unwrap();
        let d = NormConfig::default();
        assert_eq!(parsed.drop_fields, d.drop_fields);
        assert_eq!(parsed.tidy_whitespace, d.tidy_whitespace);
        assert_eq!(parsed.arxiv, d.arxiv);
    }
}

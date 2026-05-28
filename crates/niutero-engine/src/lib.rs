//! niutero-engine — the operations layer.
//!
//! Every niutero capability is a function here, expressed over an open
//! [`Vault`], so the CLI and a future GUI drive the *same* code. Front-ends
//! only parse input into the request types below and format the results;
//! nothing operational lives in a binary. This is what makes "the GUI is a
//! thin client over the CLI's interface" real rather than aspirational.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use niutero_bib::{entries, parse, BibItem};
use niutero_core::{filter, BibEntry};
use serde::{Deserialize, Serialize};

pub use niutero_vault::Vault;

/// An owned, serializable view of an entry plus its sidecar tags/notes — the
/// stable shape the CLI's `--json` emits and a GUI consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryView {
    pub citekey: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub fields: IndexMap<String, String>,
    pub tags: Vec<String>,
    pub note: String,
}

/// Which entries [`list`] returns.
pub enum Filter {
    All,
    Query(String),
    View(String),
}

/// Where [`add`] gets its entries.
pub enum AddSource {
    /// Parse raw BibTeX (one or more entries).
    Bibtex(String),
    /// Parse a BibTeX file.
    File(PathBuf),
    /// Build a single entry from explicit fields (`fields` are `NAME=VALUE`).
    Fields {
        type_: String,
        key: String,
        fields: Vec<String>,
    },
}

/// Open a folder as a vault.
pub fn open(path: &Path) -> Result<Vault, String> {
    Vault::open(path).map_err(|e| format!("open {}: {e}", path.display()))
}

/// Initialize a folder as a vault.
pub fn init(path: &Path) -> Result<Vault, String> {
    Vault::init(path).map_err(|e| format!("init {}: {e}", path.display()))
}

/// Entries matching `filter`, each with its sidecar tags/notes.
pub fn list(v: &Vault, filter: Filter) -> Result<Vec<EntryView>, String> {
    let items = read_items(v)?;
    let query = match filter {
        Filter::All => String::new(),
        Filter::Query(q) => q,
        Filter::View(name) => v
            .views
            .views
            .iter()
            .find(|w| w.name == name)
            .map(|w| w.query.clone())
            .ok_or_else(|| format!("no saved view named '{name}'"))?,
    };
    Ok(entries(&items)
        .filter(|e| {
            let tags = v
                .meta
                .get(&e.citekey)
                .map(|m| m.tags.as_slice())
                .unwrap_or(&[]);
            filter::entry_matches(&query, e, tags)
        })
        .map(|e| view_of(e, v))
        .collect())
}

/// One entry by cite key.
pub fn show(v: &Vault, citekey: &str) -> Result<EntryView, String> {
    let items = read_items(v)?;
    let found = entries(&items)
        .find(|e| e.citekey == citekey)
        .map(|e| view_of(e, v));
    found.ok_or_else(|| format!("no entry with cite key '{citekey}'"))
}

/// Add new entries. Rejects invalid entries and duplicate cite keys (existing
/// or within the batch); existing entries and verbatim blocks are preserved.
/// Returns the cite keys added.
pub fn add(v: &Vault, source: AddSource) -> Result<Vec<String>, String> {
    let mut items = read_items(v)?;
    let new_entries: Vec<BibEntry> = match source {
        AddSource::Bibtex(src) => parse_entries(&src)?,
        AddSource::File(path) => {
            let src = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            parse_entries(&src)?
        }
        AddSource::Fields { type_, key, fields } => {
            let mut e = BibEntry::new(type_, key);
            for f in &fields {
                let (name, value) = split_field(f)?;
                e.set(name, value);
            }
            vec![e]
        }
    };

    // Validate before touching disk so a corrupt entry is never written.
    for e in &new_entries {
        e.validate()?;
    }

    let mut seen: std::collections::HashSet<String> =
        entries(&items).map(|e| e.citekey.clone()).collect();
    for e in &new_entries {
        if !seen.insert(e.citekey.clone()) {
            return Err(format!(
                "cite key '{}' already exists (use `edit` to change it)",
                e.citekey
            ));
        }
    }

    let keys: Vec<String> = new_entries.iter().map(|e| e.citekey.clone()).collect();
    for e in new_entries {
        items.push(BibItem::Entry(e));
    }
    write_items(v, &items)?;
    Ok(keys)
}

/// Edit an existing entry: set fields (`NAME=VALUE`), unset fields, change the
/// type. The mutated entry is validated before writing.
pub fn edit(
    v: &Vault,
    citekey: &str,
    fields: &[String],
    unset: &[String],
    type_: Option<String>,
) -> Result<(), String> {
    let mut items = read_items(v)?;
    let idx = find_entry(&items, citekey)?;
    if let BibItem::Entry(e) = &mut items[idx] {
        if let Some(t) = type_ {
            e.entry_type = t.to_ascii_lowercase();
        }
        for f in fields {
            let (name, value) = split_field(f)?;
            e.set(name, value);
        }
        for name in unset {
            e.remove(name);
        }
    }
    if let BibItem::Entry(e) = &items[idx] {
        e.validate()?;
    }
    write_items(v, &items)
}

/// Remove an entry and clean up its sidecar metadata (writing the sidecar only
/// when something was removed).
pub fn rm(v: &mut Vault, citekey: &str) -> Result<(), String> {
    let mut items = read_items(v)?;
    let idx = find_entry(&items, citekey)?;
    items.remove(idx);
    write_items(v, &items)?;
    if v.meta.remove(citekey).is_some() {
        v.save_sidecar()
            .map_err(|e| format!("update sidecar: {e}"))?;
    }
    Ok(())
}

// ----------------------------------------------------------------- helpers

fn read_items(v: &Vault) -> Result<Vec<BibItem>, String> {
    v.read_items()
        .map_err(|e| format!("read references.bib: {e}"))
}

fn write_items(v: &Vault, items: &[BibItem]) -> Result<(), String> {
    v.write_items(items)
        .map_err(|e| format!("write references.bib: {e}"))
}

fn find_entry(items: &[BibItem], citekey: &str) -> Result<usize, String> {
    items
        .iter()
        .position(|it| matches!(it, BibItem::Entry(e) if e.citekey == citekey))
        .ok_or_else(|| format!("no entry with cite key '{citekey}'"))
}

fn view_of(entry: &BibEntry, v: &Vault) -> EntryView {
    let (tags, note) = match v.meta.get(&entry.citekey) {
        Some(m) => (m.tags.clone(), m.note.clone()),
        None => (Vec::new(), String::new()),
    };
    EntryView {
        citekey: entry.citekey.clone(),
        entry_type: entry.entry_type.clone(),
        fields: entry.fields.clone(),
        tags,
        note,
    }
}

fn split_field(s: &str) -> Result<(&str, &str), String> {
    s.split_once('=')
        .filter(|(n, _)| !n.is_empty())
        .ok_or_else(|| format!("field must be NAME=VALUE: '{s}'"))
}

fn parse_entries(src: &str) -> Result<Vec<BibEntry>, String> {
    let parsed = parse(src);
    let es: Vec<BibEntry> = entries(&parsed).cloned().collect();
    if es.is_empty() {
        Err("no BibTeX entries found in input".into())
    } else {
        Ok(es)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niutero_vault::View;

    fn fields(type_: &str, key: &str, kvs: &[&str]) -> AddSource {
        AddSource::Fields {
            type_: type_.into(),
            key: key.into(),
            fields: kvs.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let v = init(d.path()).unwrap();
        (d, v)
    }

    #[test]
    fn add_then_show() {
        let (_d, v) = vault();
        let keys = add(&v, fields("article", "k", &["title=Hi", "year=2020"])).unwrap();
        assert_eq!(keys, vec!["k".to_string()]);
        let view = show(&v, "k").unwrap();
        assert_eq!(view.entry_type, "article");
        assert_eq!(view.fields.get("title").map(String::as_str), Some("Hi"));
        assert!(view.tags.is_empty());
    }

    #[test]
    fn add_rejects_duplicate_and_invalid() {
        let (_d, v) = vault();
        add(&v, fields("misc", "dup", &[])).unwrap();
        assert!(add(&v, fields("misc", "dup", &[]))
            .unwrap_err()
            .contains("already exists"));
        assert!(add(&v, fields("misc", "x}", &[]))
            .unwrap_err()
            .contains("illegal character"));
        assert!(add(&v, fields("misc", "ok", &["title=a}"]))
            .unwrap_err()
            .contains("unbalanced"));
    }

    #[test]
    fn add_from_bibtex_canonicalizes() {
        let (_d, v) = vault();
        add(&v, AddSource::Bibtex("@MISC{b, Title=\"x\"}".into())).unwrap();
        let view = show(&v, "b").unwrap();
        assert_eq!(view.entry_type, "misc");
        assert_eq!(view.fields.get("title").map(String::as_str), Some("x"));
    }

    #[test]
    fn edit_sets_unsets_and_retypes() {
        let (_d, v) = vault();
        add(&v, fields("article", "k", &["title=Old", "year=1999"])).unwrap();
        edit(
            &v,
            "k",
            &["title=New".into()],
            &["year".into()],
            Some("inproceedings".into()),
        )
        .unwrap();
        let view = show(&v, "k").unwrap();
        assert_eq!(view.entry_type, "inproceedings");
        assert_eq!(view.fields.get("title").map(String::as_str), Some("New"));
        assert!(view.fields.get("year").is_none());
    }

    #[test]
    fn edit_missing_errors() {
        let (_d, v) = vault();
        assert!(edit(&v, "nope", &["x=1".into()], &[], None)
            .unwrap_err()
            .contains("no entry with cite key 'nope'"));
    }

    #[test]
    fn rm_removes_entry_and_meta() {
        let (_d, mut v) = vault();
        add(&v, fields("misc", "a", &[])).unwrap();
        add(&v, fields("misc", "b", &[])).unwrap();
        v.meta.insert("a".into(), Default::default());
        rm(&mut v, "a").unwrap();
        assert!(show(&v, "a").is_err());
        assert!(show(&v, "b").is_ok());
        assert!(!v.meta.contains_key("a"));
    }

    #[test]
    fn list_filters_by_query_and_view() {
        let (_d, mut v) = vault();
        add(&v, fields("article", "shannon", &["title=Theory"])).unwrap();
        add(&v, fields("inproceedings", "niu", &["title=Llama"])).unwrap();
        assert_eq!(list(&v, Filter::All).unwrap().len(), 2);
        let q = list(&v, Filter::Query("llama".into())).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].citekey, "niu");

        v.views.views.push(View {
            name: "NLP".into(),
            query: "llama".into(),
        });
        assert_eq!(list(&v, Filter::View("NLP".into())).unwrap().len(), 1);
        assert!(list(&v, Filter::View("Nope".into()))
            .unwrap_err()
            .contains("no saved view named 'Nope'"));
    }

    #[test]
    fn list_includes_sidecar_tags() {
        let (_d, mut v) = vault();
        add(&v, fields("misc", "k", &[])).unwrap();
        v.meta.insert(
            "k".into(),
            niutero_vault::EntryMeta {
                tags: vec!["nlp".into()],
                note: "n".into(),
                added: None,
            },
        );
        let view = show(&v, "k").unwrap();
        assert_eq!(view.tags, vec!["nlp".to_string()]);
        assert_eq!(view.note, "n");
    }
}

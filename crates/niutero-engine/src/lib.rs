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

pub use niutero_vault::{Vault, View};

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

// ------------------------------------------------ tags / notes (sidecar only)

/// Current tags for an entry (errors if the entry is absent).
pub fn current_tags(v: &Vault, citekey: &str) -> Result<Vec<String>, String> {
    entry_exists(v, citekey)?;
    Ok(v.meta
        .get(citekey)
        .map(|m| m.tags.clone())
        .unwrap_or_default())
}

/// Add and/or remove tags on an entry; returns the resulting tag set (sorted,
/// deduped). Writes only the sidecar — `references.bib` is never touched.
pub fn set_tags(
    v: &mut Vault,
    citekey: &str,
    add: &[String],
    remove: &[String],
) -> Result<Vec<String>, String> {
    entry_exists(v, citekey)?;
    {
        let meta = v.meta.entry(citekey.to_string()).or_default();
        for t in add {
            if !t.is_empty() && !meta.tags.iter().any(|x| x == t) {
                meta.tags.push(t.clone());
            }
        }
        meta.tags.retain(|t| !remove.iter().any(|r| r == t));
        meta.tags.sort();
        meta.tags.dedup();
    }
    let result = v
        .meta
        .get(citekey)
        .map(|m| m.tags.clone())
        .unwrap_or_default();
    prune_meta(v, citekey);
    save_sidecar(v)?;
    Ok(result)
}

/// Current note for an entry (empty string if none; errors if the entry is absent).
pub fn current_note(v: &Vault, citekey: &str) -> Result<String, String> {
    entry_exists(v, citekey)?;
    Ok(v.meta
        .get(citekey)
        .map(|m| m.note.clone())
        .unwrap_or_default())
}

/// Set (`Some`) or clear (`None`) an entry's note. Sidecar only.
pub fn set_note(v: &mut Vault, citekey: &str, note: Option<String>) -> Result<(), String> {
    entry_exists(v, citekey)?;
    v.meta.entry(citekey.to_string()).or_default().note = note.unwrap_or_default();
    prune_meta(v, citekey);
    save_sidecar(v)
}

// ------------------------------------------------------------- saved views

/// All saved views, in order.
pub fn views(v: &Vault) -> &[View] {
    &v.views.views
}

/// Add a saved view; errors if the name is already taken.
pub fn add_view(v: &mut Vault, name: String, query: String) -> Result<(), String> {
    if v.views.views.iter().any(|w| w.name == name) {
        return Err(format!("a view named '{name}' already exists"));
    }
    v.views.views.push(View { name, query });
    save_sidecar(v)
}

/// Remove a saved view by name; errors if there is no such view.
pub fn remove_view(v: &mut Vault, name: &str) -> Result<(), String> {
    let before = v.views.views.len();
    v.views.views.retain(|w| w.name != name);
    if v.views.views.len() == before {
        return Err(format!("no saved view named '{name}'"));
    }
    save_sidecar(v)
}

// ----------------------------------------------------------------- helpers

fn entry_exists(v: &Vault, citekey: &str) -> Result<(), String> {
    let items = read_items(v)?;
    if entries(&items).any(|e| e.citekey == citekey) {
        Ok(())
    } else {
        Err(format!("no entry with cite key '{citekey}'"))
    }
}

/// Drop the meta entry if it carries nothing, keeping `meta.json` minimal.
fn prune_meta(v: &mut Vault, citekey: &str) {
    if v.meta.get(citekey).is_some_and(|m| m.is_empty()) {
        v.meta.remove(citekey);
    }
}

fn save_sidecar(v: &Vault) -> Result<(), String> {
    v.save_sidecar().map_err(|e| format!("update sidecar: {e}"))
}

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

    #[test]
    fn tags_add_remove_sorted_deduped_pruned_and_bib_untouched() {
        let (_d, mut v) = vault();
        add(&v, fields("misc", "k", &[])).unwrap();
        let bib_before = std::fs::read_to_string(v.bib_path()).unwrap();

        let tags = set_tags(
            &mut v,
            "k",
            &["nlp".into(), "llm".into(), "nlp".into()],
            &[],
        )
        .unwrap();
        assert_eq!(tags, vec!["llm".to_string(), "nlp".to_string()]);

        let tags = set_tags(&mut v, "k", &[], &["llm".into(), "nlp".into()]).unwrap();
        assert!(tags.is_empty());
        assert!(!v.meta.contains_key("k"), "empty meta should be pruned");

        // Tagging never rewrites the source of truth.
        assert_eq!(std::fs::read_to_string(v.bib_path()).unwrap(), bib_before);
    }

    #[test]
    fn note_set_clear_and_prune() {
        let (_d, mut v) = vault();
        add(&v, fields("misc", "k", &[])).unwrap();
        set_note(&mut v, "k", Some("seminal".into())).unwrap();
        assert_eq!(current_note(&v, "k").unwrap(), "seminal");
        set_note(&mut v, "k", None).unwrap();
        assert_eq!(current_note(&v, "k").unwrap(), "");
        assert!(!v.meta.contains_key("k"));
    }

    #[test]
    fn tag_and_note_require_existing_entry() {
        let (_d, mut v) = vault();
        assert!(set_tags(&mut v, "ghost", &["x".into()], &[])
            .unwrap_err()
            .contains("no entry with cite key 'ghost'"));
        assert!(set_note(&mut v, "ghost", Some("n".into()))
            .unwrap_err()
            .contains("no entry"));
    }

    #[test]
    fn views_add_list_remove() {
        let (_d, mut v) = vault();
        add_view(&mut v, "NLP".into(), "tag:nlp".into()).unwrap();
        assert_eq!(views(&v).len(), 1);
        assert_eq!(views(&v)[0].query, "tag:nlp");
        assert!(add_view(&mut v, "NLP".into(), "x".into())
            .unwrap_err()
            .contains("already exists"));
        remove_view(&mut v, "NLP").unwrap();
        assert!(views(&v).is_empty());
        assert!(remove_view(&mut v, "NLP")
            .unwrap_err()
            .contains("no saved view named 'NLP'"));
    }
}

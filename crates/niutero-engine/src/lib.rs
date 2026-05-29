//! niutero-engine — the operations layer.
//!
//! Every niutero capability is a function here, expressed over an open
//! [`Vault`], so the CLI and a future GUI drive the *same* code. Front-ends
//! only parse input into the request types below and format the results;
//! nothing operational lives in a binary. This is what makes "the GUI is a
//! thin client over the CLI's interface" real rather than aspirational.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use niutero_bib::{entries, parse, to_bibtex_entries, BibItem};
use niutero_core::{filter, texscan, BibEntry};
use niutero_norm::{normalize_entry, NormConfig};
use niutero_sync as git;
use serde::{Deserialize, Serialize};

pub use niutero_core::texscan::TexReport;
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
    let query = resolve_query(v, filter)?;
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

// ------------------------------------------------------- import / export

/// What to do when an imported entry's cite key already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupPolicy {
    /// Keep the existing entry; drop the imported one.
    Skip,
    /// Replace the existing entry (in place) with the imported one.
    Overwrite,
    /// Add the imported entry under a fresh, unique cite key.
    Rename,
}

/// Outcome of an [`import`].
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
    pub overwritten: usize,
    /// `(original, new)` for each renamed entry.
    pub renamed: Vec<(String, String)>,
}

/// Merge the entries of an external `.bib` into the library under `policy`.
/// The whole import is atomic: if any entry is invalid the library is left
/// untouched (validation happens before the single write). Existing entries
/// and verbatim blocks are preserved.
pub fn import(v: &Vault, file: &Path, policy: DupPolicy) -> Result<ImportReport, String> {
    let src = std::fs::read_to_string(file).map_err(|e| format!("read {}: {e}", file.display()))?;
    let incoming: Vec<BibEntry> = entries(&parse(&src)).cloned().collect();
    if incoming.is_empty() {
        return Err("no BibTeX entries found in the file".into());
    }

    let mut items = read_items(v)?;
    let mut keys: std::collections::HashSet<String> =
        entries(&items).map(|e| e.citekey.clone()).collect();
    let mut report = ImportReport::default();

    for mut entry in incoming {
        if keys.contains(&entry.citekey) {
            match policy {
                DupPolicy::Skip => report.skipped += 1,
                DupPolicy::Overwrite => {
                    entry.validate()?;
                    let idx = find_entry(&items, &entry.citekey)?;
                    items[idx] = BibItem::Entry(entry);
                    report.overwritten += 1;
                }
                DupPolicy::Rename => {
                    let original = entry.citekey.clone();
                    let new = unique_key(&original, &keys);
                    entry.citekey = new.clone();
                    entry.validate()?;
                    keys.insert(new.clone());
                    items.push(BibItem::Entry(entry));
                    report.renamed.push((original, new));
                }
            }
        } else {
            entry.validate()?;
            keys.insert(entry.citekey.clone());
            items.push(BibItem::Entry(entry));
            report.added += 1;
        }
    }

    write_items(v, &items)?;
    Ok(report)
}

/// Write the entries matching `filter` to a standalone `.bib` at `out`.
/// Returns the number of entries written.
pub fn export(v: &Vault, filter: Filter, out: &Path) -> Result<usize, String> {
    let items = read_items(v)?;
    let query = resolve_query(v, filter)?;
    let selected: Vec<BibEntry> = entries(&items)
        .filter(|e| {
            let tags = v
                .meta
                .get(&e.citekey)
                .map(|m| m.tags.as_slice())
                .unwrap_or(&[]);
            filter::entry_matches(&query, e, tags)
        })
        .cloned()
        .collect();
    std::fs::write(out, to_bibtex_entries(&selected))
        .map_err(|e| format!("write {}: {e}", out.display()))?;
    Ok(selected.len())
}

// ------------------------------------------------------------- LaTeX glue

/// Scan `.tex`/`.aux` files and report cite-key usage against the library.
pub fn tex_scan(v: &Vault, tex_files: &[PathBuf]) -> Result<TexReport, String> {
    let items = read_items(v)?;
    let lib_keys: std::collections::BTreeSet<String> =
        entries(&items).map(|e| e.citekey.clone()).collect();
    let mut cited = texscan::Cited::default();
    for path in tex_files {
        let src =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let c = texscan::cited_keys(&src);
        cited.keys.extend(c.keys);
        cited.cite_all |= c.cite_all;
    }
    Ok(texscan::report(&lib_keys, &cited))
}

/// Write only the entries whose cite key is in `keys` to a standalone `.bib`
/// (e.g. a pruned bibliography for a paper). Returns the number written.
pub fn export_keys(v: &Vault, keys: &[String], out: &Path) -> Result<usize, String> {
    let items = read_items(v)?;
    let wanted: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
    let selected: Vec<BibEntry> = entries(&items)
        .filter(|e| wanted.contains(e.citekey.as_str()))
        .cloned()
        .collect();
    std::fs::write(out, to_bibtex_entries(&selected))
        .map_err(|e| format!("write {}: {e}", out.display()))?;
    Ok(selected.len())
}

// ------------------------------------------------------------------- sync

/// Outcome of a [`sync`].
#[derive(Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// Pulled and pushed cleanly; `committed` is whether local changes were committed.
    Synced { committed: bool },
    /// A pull conflict was hit and the merge aborted — the user must resolve it.
    Conflict,
}

/// Initialize git in the vault (if needed) and point `origin` at `url`.
pub fn connect(v: &Vault, url: &str) -> Result<(), String> {
    if !git::is_repo(&v.root) {
        git::init(&v.root)?;
    }
    git::set_remote(&v.root, "origin", url)
}

/// Commit local changes, pull, then push. Requires a git repo with an `origin`
/// remote (set up via [`connect`]). A pull conflict aborts and returns
/// [`SyncStatus::Conflict`] rather than leaving a half-merged tree.
pub fn sync(v: &Vault, message: Option<String>) -> Result<SyncStatus, String> {
    if !git::is_repo(&v.root) {
        return Err("not a git repository — run `niutero connect <url>` first".into());
    }
    if git::remote_url(&v.root, "origin").is_none() {
        return Err("no 'origin' remote — run `niutero connect <url>` first".into());
    }
    let message = message.unwrap_or_else(|| "niutero: sync".to_string());
    let committed = git::commit_all(&v.root, &message)?;
    if git::has_upstream(&v.root) && git::pull(&v.root)? == git::PullOutcome::Conflict {
        return Ok(SyncStatus::Conflict);
    }
    git::push(&v.root)?;
    Ok(SyncStatus::Synced { committed })
}

// -------------------------------------------------------------- normalize

/// One entry that offline normalization would change, with human-readable notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormChange {
    pub citekey: String,
    pub notes: Vec<String>,
}

/// Preview offline normalization — compute what would change without writing.
pub fn normalize_preview(v: &Vault) -> Result<Vec<NormChange>, String> {
    let items = read_items(v)?;
    let cfg = NormConfig::load(&v.niutero_dir());
    Ok(norm_changes(&items, &cfg).1)
}

/// Apply offline normalization, writing the result (only if something changed).
/// Returns the changes that were applied.
pub fn normalize_apply(v: &Vault) -> Result<Vec<NormChange>, String> {
    let items = read_items(v)?;
    let cfg = NormConfig::load(&v.niutero_dir());
    let (normalized, changes) = norm_changes(&items, &cfg);
    if !changes.is_empty() {
        write_items(v, &normalized)?;
    }
    Ok(changes)
}

fn norm_changes(items: &[BibItem], cfg: &NormConfig) -> (Vec<BibItem>, Vec<NormChange>) {
    let mut out = Vec::with_capacity(items.len());
    let mut changes = Vec::new();
    for it in items {
        match it {
            BibItem::Entry(e) => {
                let (normalized, notes) = normalize_entry(e, cfg);
                if !notes.is_empty() {
                    changes.push(NormChange {
                        citekey: e.citekey.clone(),
                        notes,
                    });
                }
                out.push(BibItem::Entry(normalized));
            }
            BibItem::Verbatim(s) => out.push(BibItem::Verbatim(s.clone())),
        }
    }
    (out, changes)
}

// ----------------------------------------------------------------- helpers

fn resolve_query(v: &Vault, filter: Filter) -> Result<String, String> {
    Ok(match filter {
        Filter::All => String::new(),
        Filter::Query(q) => q,
        Filter::View(name) => v
            .views
            .views
            .iter()
            .find(|w| w.name == name)
            .map(|w| w.query.clone())
            .ok_or_else(|| format!("no saved view named '{name}'"))?,
    })
}

/// `base-2`, `base-3`, … — the first that is not already taken.
fn unique_key(base: &str, taken: &std::collections::HashSet<String>) -> String {
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|k| !taken.contains(k))
        .expect("infinite range yields an unused key")
}

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

    fn write_file(d: &tempfile::TempDir, name: &str, contents: &str) -> std::path::PathBuf {
        let p = d.path().join(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    fn title(v: &Vault, key: &str) -> Option<String> {
        show(v, key).unwrap().fields.get("title").cloned()
    }

    #[test]
    fn import_skip_keeps_existing() {
        let (d, v) = vault();
        add(&v, fields("misc", "k", &["title=Orig"])).unwrap();
        let f = write_file(
            &d,
            "in.bib",
            "@misc{k, title = {New}}\n@misc{fresh, title = {F}}\n",
        );
        let r = import(&v, &f, DupPolicy::Skip).unwrap();
        assert_eq!(r.added, 1);
        assert_eq!(r.skipped, 1);
        assert_eq!(title(&v, "k").as_deref(), Some("Orig"));
        assert!(show(&v, "fresh").is_ok());
    }

    #[test]
    fn import_overwrite_replaces_in_place() {
        let (d, v) = vault();
        add(&v, fields("misc", "k", &["title=Orig"])).unwrap();
        let f = write_file(&d, "in.bib", "@article{k, title = {New}}\n");
        let r = import(&v, &f, DupPolicy::Overwrite).unwrap();
        assert_eq!(r.overwritten, 1);
        let view = show(&v, "k").unwrap();
        assert_eq!(view.entry_type, "article");
        assert_eq!(view.fields.get("title").map(String::as_str), Some("New"));
    }

    #[test]
    fn import_rename_keeps_both() {
        let (d, v) = vault();
        add(&v, fields("misc", "k", &["title=Orig"])).unwrap();
        let f = write_file(&d, "in.bib", "@misc{k, title = {New}}\n");
        let r = import(&v, &f, DupPolicy::Rename).unwrap();
        assert_eq!(r.renamed, vec![("k".to_string(), "k-2".to_string())]);
        assert_eq!(title(&v, "k").as_deref(), Some("Orig"));
        assert_eq!(title(&v, "k-2").as_deref(), Some("New"));
    }

    #[test]
    fn import_is_atomic_on_invalid_entry() {
        let (d, v) = vault();
        add(&v, fields("misc", "keep", &[])).unwrap();
        let before = std::fs::read_to_string(v.bib_path()).unwrap();
        let f = write_file(
            &d,
            "in.bib",
            "@misc{good, title={G}}\n@misc{bad key, title={B}}\n",
        );
        assert!(import(&v, &f, DupPolicy::Skip).is_err());
        // Nothing was written: the bad cite key aborted the whole import.
        assert_eq!(std::fs::read_to_string(v.bib_path()).unwrap(), before);
    }

    #[test]
    fn import_no_entries_errors() {
        let (d, v) = vault();
        let f = write_file(&d, "empty.bib", "% just a comment\n");
        assert!(import(&v, &f, DupPolicy::Skip)
            .unwrap_err()
            .contains("no BibTeX entries"));
    }

    #[test]
    fn import_preserves_existing_verbatim() {
        let (d, v) = vault();
        v.write_items(&parse("@string{acl = {ACL}}\n\n@misc{k}\n"))
            .unwrap();
        let f = write_file(&d, "in.bib", "@misc{new1, title={N}}\n");
        import(&v, &f, DupPolicy::Skip).unwrap();
        let bib = std::fs::read_to_string(v.bib_path()).unwrap();
        assert!(bib.contains("@string{acl = {ACL}}"));
        assert!(bib.contains("@misc{new1"));
    }

    #[test]
    fn export_all_query_and_count() {
        let (d, v) = vault();
        add(&v, fields("article", "a", &["title=Apple"])).unwrap();
        add(&v, fields("misc", "b", &["title=Banana"])).unwrap();
        let out = d.path().join("out.bib");

        assert_eq!(export(&v, Filter::All, &out).unwrap(), 2);

        assert_eq!(export(&v, Filter::Query("apple".into()), &out).unwrap(), 1);
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(written.contains("@article{a,"));
        assert!(!written.contains("@misc{b,"));
    }

    #[test]
    fn tex_scan_reports_and_export_keys_prunes() {
        let (d, v) = vault();
        add(&v, fields("article", "used1", &["title=U1"])).unwrap();
        add(&v, fields("misc", "unused1", &["title=N"])).unwrap();
        let tex = write_file(&d, "paper.tex", r"\cite{used1,missing1}");
        let report = tex_scan(&v, &[tex]).unwrap();
        assert_eq!(report.used, vec!["used1".to_string()]);
        assert_eq!(report.missing, vec!["missing1".to_string()]);
        assert_eq!(report.unused, vec!["unused1".to_string()]);

        let out = d.path().join("cited.bib");
        assert_eq!(export_keys(&v, &report.used, &out).unwrap(), 1);
        let w = std::fs::read_to_string(&out).unwrap();
        assert!(w.contains("@article{used1,"));
        assert!(!w.contains("unused1"));
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
    }

    fn git_identity(dir: &std::path::Path) {
        run_git(dir, &["config", "user.email", "t@e.com"]);
        run_git(dir, &["config", "user.name", "T"]);
        run_git(dir, &["config", "commit.gpgsign", "false"]);
    }

    fn git_clone(bare: &str, dst: &std::path::Path) {
        std::process::Command::new("git")
            .args(["clone", bare, dst.to_str().unwrap()])
            .output()
            .unwrap();
    }

    #[test]
    fn sync_without_connect_errors() {
        let (_d, v) = vault();
        assert!(sync(&v, None).unwrap_err().contains("not a git repository"));
    }

    #[test]
    fn sync_pushes_and_a_clone_sees_it() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, v) = vault();
        connect(&v, bare).unwrap();
        git_identity(&v.root);
        add(&v, fields("misc", "k", &["title=Hi"])).unwrap();
        assert_eq!(
            sync(&v, None).unwrap(),
            SyncStatus::Synced { committed: true }
        );

        let dst = tempfile::tempdir().unwrap();
        let clone = dst.path().join("clone");
        git_clone(bare, &clone);
        let bib = std::fs::read_to_string(clone.join("references.bib")).unwrap();
        assert!(bib.contains("@misc{k,"));
    }

    #[test]
    fn sync_detects_conflict() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        // A: connect, add, push.
        let (_da, a) = vault();
        connect(&a, bare).unwrap();
        git_identity(&a.root);
        add(&a, fields("misc", "k", &["title=A"])).unwrap();
        sync(&a, None).unwrap();

        // B: clone the pushed repo.
        let dstb = tempfile::tempdir().unwrap();
        let b_path = dstb.path().join("b");
        git_clone(bare, &b_path);
        git_identity(&b_path);
        let b = open(&b_path).unwrap();

        // A and B change the same line differently; B's sync conflicts.
        edit(&a, "k", &["title=A2".into()], &[], None).unwrap();
        sync(&a, None).unwrap();
        edit(&b, "k", &["title=B2".into()], &[], None).unwrap();
        assert_eq!(sync(&b, None).unwrap(), SyncStatus::Conflict);
    }

    #[test]
    fn normalize_previews_then_applies_idempotently() {
        let (_d, v) = vault();
        add(
            &v,
            AddSource::Bibtex("@article{k, title={A  B}, abstract={x}}".into()),
        )
        .unwrap();
        let before = std::fs::read_to_string(v.bib_path()).unwrap();

        // preview reports the change but does not write
        let changes = normalize_preview(&v).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].citekey, "k");
        assert_eq!(std::fs::read_to_string(v.bib_path()).unwrap(), before);

        // apply writes; the noise field is gone and whitespace tidied
        assert_eq!(normalize_apply(&v).unwrap().len(), 1);
        let view = show(&v, "k").unwrap();
        assert!(view.fields.get("abstract").is_none());
        assert_eq!(view.fields.get("title").map(String::as_str), Some("A B"));

        // idempotent: nothing left to change
        assert!(normalize_preview(&v).unwrap().is_empty());
    }

    #[test]
    fn normalize_respects_norm_toml() {
        let (_d, v) = vault();
        add(
            &v,
            AddSource::Bibtex("@article{k, title={A  B}, abstract={x}}".into()),
        )
        .unwrap();
        std::fs::write(
            v.niutero_dir().join("norm.toml"),
            "drop_fields = []\ntidy_whitespace = false\narxiv = false\n",
        )
        .unwrap();
        assert!(normalize_preview(&v).unwrap().is_empty());
    }
}

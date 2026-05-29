//! niutero-engine — the operations layer.
//!
//! Every niutero capability is a function here, expressed over an open
//! [`Vault`], so the CLI and a future GUI drive the *same* code. Front-ends
//! only parse input into the request types below and format the results;
//! nothing operational lives in a binary. This is what makes "the GUI is a
//! thin client over the CLI's interface" real rather than aspirational.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use niutero_bib::{entries, entry_line_span, parse, to_bibtex_entries, BibItem};
use niutero_core::filter::Facets;
use niutero_core::{dedup, filter, texscan, BibEntry, KeyPattern};
use niutero_norm::{normalize_entry, NormConfig};
use niutero_sync as git;
use serde::{Deserialize, Serialize};

pub use niutero_core::texscan::TexReport;
pub use niutero_vault::{EntryMeta, Status, Vault, View};

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
    /// Reading status name (`unread` / `reading` / `done`).
    pub status: String,
    /// Star rating 1–5, or `None` if unrated.
    pub stars: Option<u8>,
    pub added: Option<String>,
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

/// Initialize a folder as a vault, scaffolding a README and a default
/// `norm.toml` (both only if absent).
pub fn init(path: &Path) -> Result<Vault, String> {
    let v = Vault::init(path).map_err(|e| format!("init {}: {e}", path.display()))?;
    write_readme_if_absent(&v).map_err(|e| format!("write README.md: {e}"))?;
    niutero_norm::NormConfig::write_default_if_absent(&v.niutero_dir())
        .map_err(|e| format!("write norm.toml: {e}"))?;
    Ok(v)
}

/// Entries matching `filter`, each with its sidecar tags/notes.
pub fn list(v: &Vault, filter: Filter) -> Result<Vec<EntryView>, String> {
    let items = read_items(v)?;
    let query = resolve_query(v, filter)?;
    Ok(entries(&items)
        .filter(|e| filter::entry_matches(&query, e, &facets_of(v, &e.citekey)))
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
    let _lock = lock_vault(v)?;
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
            // No cite key given: generate one from the library's pattern.
            if e.citekey.is_empty() {
                e.citekey = generate_citekey(v, &e, &items);
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
    let _lock = lock_vault(v)?;
    let mut items = read_items(v)?;
    let idx = find_entry(&items, citekey)?;
    if let BibItem::Entry(e) = &mut items[idx] {
        if let Some(t) = type_ {
            e.set_type(t);
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
    let _lock = lock_vault(v)?;
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
    let _lock = lock_vault(v)?;
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
    let _lock = lock_vault(v)?;
    entry_exists(v, citekey)?;
    v.meta.entry(citekey.to_string()).or_default().note = note.unwrap_or_default();
    prune_meta(v, citekey);
    save_sidecar(v)
}

/// Set an entry's reading status. `Unread` is the default and is stored as
/// *absent* so `meta.json` stays minimal. Sidecar only.
pub fn set_status(v: &mut Vault, citekey: &str, status: Status) -> Result<(), String> {
    let _lock = lock_vault(v)?;
    entry_exists(v, citekey)?;
    let stored = (status != Status::Unread).then_some(status);
    v.meta.entry(citekey.to_string()).or_default().status = stored;
    prune_meta(v, citekey);
    save_sidecar(v)
}

/// Set (`1..=5`) or clear (`0`/`None`) an entry's star rating. Rejects ratings
/// above 5. Sidecar only.
pub fn set_stars(v: &mut Vault, citekey: &str, stars: Option<u8>) -> Result<(), String> {
    let _lock = lock_vault(v)?;
    entry_exists(v, citekey)?;
    let stored = match stars {
        Some(0) | None => None,
        Some(n) if n <= 5 => Some(n),
        Some(n) => return Err(format!("stars must be between 0 and 5, got {n}")),
    };
    v.meta.entry(citekey.to_string()).or_default().stars = stored;
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
    let _lock = lock_vault(v)?;
    if v.views.views.iter().any(|w| w.name == name) {
        return Err(format!("a view named '{name}' already exists"));
    }
    v.views.views.push(View { name, query });
    save_sidecar(v)
}

/// Remove a saved view by name; errors if there is no such view.
pub fn remove_view(v: &mut Vault, name: &str) -> Result<(), String> {
    let _lock = lock_vault(v)?;
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

/// Merge the entries of an external `.bib` file into the library under
/// `policy`. The whole import is atomic: if any entry is invalid the library is
/// left untouched (validation happens before the single write). Existing
/// entries and verbatim blocks are preserved.
pub fn import(v: &Vault, file: &Path, policy: DupPolicy) -> Result<ImportReport, String> {
    let _lock = lock_vault(v)?;
    let src = std::fs::read_to_string(file).map_err(|e| format!("read {}: {e}", file.display()))?;
    let incoming: Vec<BibEntry> = entries(&parse(&src)).cloned().collect();
    if incoming.is_empty() {
        return Err("no BibTeX entries found in the file".into());
    }
    merge_incoming(v, incoming, policy)
}

/// **Online.** Fetch an entry's BibTeX from its DOI (via doi.org) and import it
/// under `policy`. Needs network access; the offline core never calls this.
pub fn import_doi(v: &Vault, doi: &str, policy: DupPolicy) -> Result<ImportReport, String> {
    let _lock = lock_vault(v)?;
    let src = niutero_online::fetch_doi_bibtex(doi)?;
    let incoming: Vec<BibEntry> = entries(&parse(&src)).cloned().collect();
    if incoming.is_empty() {
        return Err(format!("doi.org returned no BibTeX entries for {doi}"));
    }
    merge_incoming(v, incoming, policy)
}

/// Merge `incoming` entries into the library under `policy`. The caller must
/// already hold the vault lock.
fn merge_incoming(
    v: &Vault,
    incoming: Vec<BibEntry>,
    policy: DupPolicy,
) -> Result<ImportReport, String> {
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

// -------------------------------------------------------------- online enrich

/// **Online.** Fill in fields an entry is missing by fetching its canonical
/// record from its DOI (doi.org). Never overwrites a field the entry already
/// has with a non-blank value. Returns the names of the fields that were
/// filled. The entry must carry a DOI (a `doi` field, or a doi.org `url`).
pub fn enrich(v: &Vault, citekey: &str) -> Result<Vec<String>, String> {
    let _lock = lock_vault(v)?;
    let mut items = read_items(v)?;
    let idx = find_entry(&items, citekey)?;
    let doi = match &items[idx] {
        BibItem::Entry(e) => entry_doi(e),
        BibItem::Verbatim(_) => None,
    }
    .ok_or_else(|| format!("'{citekey}' has no DOI to enrich from"))?;

    let src = niutero_online::fetch_doi_bibtex(&doi)?;
    let fetched = entries(&parse(&src))
        .next()
        .cloned()
        .ok_or_else(|| format!("doi.org returned no BibTeX for {doi}"))?;

    let filled = match &mut items[idx] {
        BibItem::Entry(e) => fill_missing(e, &fetched),
        BibItem::Verbatim(_) => Vec::new(),
    };
    if !filled.is_empty() {
        if let BibItem::Entry(e) = &items[idx] {
            e.validate()?;
        }
        write_items(v, &items)?;
    }
    Ok(filled)
}

/// The DOI to enrich `entry` from: its `doi` field, else a DOI parsed out of a
/// doi.org `url`. Pure.
fn entry_doi(entry: &BibEntry) -> Option<String> {
    if let Some(doi) = entry.get("doi").map(str::trim).filter(|d| !d.is_empty()) {
        return Some(doi.to_string());
    }
    let url = entry.get("url")?.trim();
    url.strip_prefix("https://doi.org/")
        .or_else(|| url.strip_prefix("http://doi.org/"))
        .map(|d| d.trim_matches('/').to_string())
        .filter(|d| !d.is_empty())
}

/// Copy any field present on `fetched` but missing (or blank) on `entry`,
/// without overwriting what the entry already has. Returns the filled names.
/// Pure.
fn fill_missing(entry: &mut BibEntry, fetched: &BibEntry) -> Vec<String> {
    let mut filled = Vec::new();
    for (name, value) in &fetched.fields {
        let have = entry.get(name).is_some_and(|v| !v.trim().is_empty());
        if !have && !value.trim().is_empty() {
            entry.set(name, value);
            filled.push(name.clone());
        }
    }
    filled
}

/// Write the entries matching `filter` to a standalone `.bib` at `out`.
/// Returns the number of entries written.
pub fn export(v: &Vault, filter: Filter, out: &Path) -> Result<usize, String> {
    let items = read_items(v)?;
    let query = resolve_query(v, filter)?;
    let selected: Vec<BibEntry> = entries(&items)
        .filter(|e| filter::entry_matches(&query, e, &facets_of(v, &e.citekey)))
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

/// `\cite{key}` for an entry (errors if the cite key is absent).
pub fn cite(v: &Vault, citekey: &str) -> Result<String, String> {
    entry_exists(v, citekey)?;
    Ok(format!("\\cite{{{citekey}}}"))
}

// ------------------------------------------------------------------- sync

/// Outcome of a [`sync`].
#[derive(Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// Pulled and pushed cleanly. `committed` = local changes were committed;
    /// `merged` = a pull conflict was auto-resolved by a structured 3-way merge.
    Synced { committed: bool, merged: bool },
    /// A pull conflict could not be auto-resolved (real field conflict, or a
    /// non-`references.bib` file conflicted); the merge was aborted.
    Conflict,
}

/// Initialize git in the vault (if needed), point `origin` at `url`, and pin
/// the repo's line-ending behavior so `references.bib` stays byte-identical
/// across platforms (the source-of-truth invariant).
pub fn connect(v: &Vault, url: &str) -> Result<(), String> {
    let _lock = lock_vault(v)?;
    if !git::is_repo(&v.root) {
        git::init(&v.root)?;
    }
    git::set_remote(&v.root, "origin", url)?;
    ensure_repo_hygiene(v)
}

/// What every niutero repo needs to keep the `.bib` byte-stable: a
/// `.gitattributes` forcing LF, and `core.autocrlf=false` locally. Idempotent
/// and safe to call on every sync.
fn ensure_repo_hygiene(v: &Vault) -> Result<(), String> {
    let path = v.root.join(".gitattributes");
    if !path.exists() {
        std::fs::write(path, GITATTRIBUTES).map_err(|e| format!("write .gitattributes: {e}"))?;
    }
    // gitattributes is the real guarantee; autocrlf is belt-and-suspenders, so
    // don't fail the whole operation if this one config write hiccups.
    let _ = git::set_config(&v.root, "core.autocrlf", "false");
    Ok(())
}

/// `references.bib` (and everything else) is committed with LF endings on every
/// platform, so the source of truth never churns on a Windows checkout.
const GITATTRIBUTES: &str = "* text=auto eol=lf\n";

/// Commit local changes, pull, then push. Requires a git repo with an `origin`
/// remote (set up via [`connect`]). On a pull conflict it attempts a structured
/// entry-level 3-way merge of `references.bib`; if that resolves cleanly the
/// merge is committed and the sync proceeds, otherwise the merge is aborted and
/// [`SyncStatus::Conflict`] is returned (never a half-merged tree).
pub fn sync(v: &Vault, message: Option<String>) -> Result<SyncStatus, String> {
    let _lock = lock_vault(v)?;
    if !git::is_repo(&v.root) {
        return Err("not a git repository — run `niutero connect <url>` first".into());
    }
    if git::remote_url(&v.root, "origin").is_none() {
        return Err("no 'origin' remote — run `niutero connect <url>` first".into());
    }
    ensure_repo_hygiene(v)?;
    let message = message.unwrap_or_else(|| auto_commit_message(v));
    let committed = git::commit_all(&v.root, &message)?;
    let mut merged = false;
    if git::has_upstream(&v.root) && git::pull(&v.root)? == git::PullOutcome::Conflict {
        // A merge is now in progress; resolve it or leave a clean tree.
        match try_resolve_merge(v) {
            Ok(true) => merged = true,
            Ok(false) => {
                git::abort_merge(&v.root)?;
                return Ok(SyncStatus::Conflict);
            }
            Err(e) => {
                let _ = git::abort_merge(&v.root); // best effort; surface the real error
                return Err(e);
            }
        }
    }
    git::push(&v.root)?;
    Ok(SyncStatus::Synced { committed, merged })
}

/// Try to auto-resolve an in-progress merge by 3-way merging the *entries* of
/// `references.bib` (the structured resolver). Returns `Ok(true)` if it resolved
/// and committed the merge, `Ok(false)` if it can't — in which case the caller
/// aborts so the user resolves by hand.
///
/// It is deliberately conservative: it auto-resolves only when it can prove the
/// result is correct, and bails (Ok(false)) otherwise. It declines when
/// * anything other than `references.bib` conflicted (e.g. a sidecar);
/// * the file was modify/deleted whole on a side (a stage is missing) — taking a
///   missing stage as an empty library would silently delete the other side's
///   entries;
/// * the two sides disagree on the verbatim blocks (`@string`/`@preamble`/
///   `@comment`/free text). Those aren't entry-keyed, so we only merge entries
///   when both sides left the verbatim identical — never silently picking or
///   duplicating a `@string`;
/// * the entry-level 3-way merge reports any conflict.
fn try_resolve_merge(v: &Vault) -> Result<bool, String> {
    let conflicted = git::conflicted_paths(&v.root);
    if conflicted.iter().any(|p| p != "references.bib") || conflicted.is_empty() {
        return Ok(false);
    }

    // Require all three stages as real blobs. A missing stage means one side
    // deleted the whole file (a modify/delete), which we must not auto-resolve.
    let (Some(base_txt), Some(ours_txt), Some(theirs_txt)) = (
        git::merge_stage(&v.root, 1, "references.bib"),
        git::merge_stage(&v.root, 2, "references.bib"),
        git::merge_stage(&v.root, 3, "references.bib"),
    ) else {
        return Ok(false);
    };
    let (base_items, ours_items, theirs_items) =
        (parse(&base_txt), parse(&ours_txt), parse(&theirs_txt));

    // Verbatim blocks have no cite key to merge on, so only proceed when both
    // sides agree on them; otherwise the verbatim is itself in conflict.
    let verbatim = |items: &[BibItem]| -> Vec<String> {
        items
            .iter()
            .filter_map(|it| match it {
                BibItem::Verbatim(s) => Some(s.clone()),
                BibItem::Entry(_) => None,
            })
            .collect()
    };
    let ours_verbatim = verbatim(&ours_items);
    if ours_verbatim != verbatim(&theirs_items) {
        return Ok(false);
    }

    let collect = |items: &[BibItem]| entries(items).cloned().collect::<Vec<BibEntry>>();
    let result = niutero_core::merge::merge(
        &collect(&base_items),
        &collect(&ours_items),
        &collect(&theirs_items),
    );
    if !result.is_clean() {
        return Ok(false);
    }

    // Rebuild: the (agreed-upon) verbatim blocks, then the merged entries.
    let mut items: Vec<BibItem> = ours_items
        .into_iter()
        .filter(|it| matches!(it, BibItem::Verbatim(_)))
        .collect();
    items.extend(result.merged.into_iter().map(BibItem::Entry));
    write_items(v, &items)?;
    git::finalize_merge(&v.root)?;
    Ok(true)
}

/// A commit message describing what changed in `references.bib` since `HEAD`,
/// at the granularity of entries (e.g. `niutero: 3 added, 1 changed`).
fn auto_commit_message(v: &Vault) -> String {
    let working = std::fs::read_to_string(v.bib_path()).unwrap_or_default();
    match git::file_at_head(&v.root, "references.bib") {
        None => "niutero: initial import".to_string(),
        Some(head) => entry_diff(&head, &working).message(),
    }
}

/// Entry-level delta between two `.bib` texts (by cite key + content equality).
struct EntryDiff {
    added: usize,
    changed: usize,
    removed: usize,
}

impl EntryDiff {
    fn message(&self) -> String {
        let mut parts = Vec::new();
        if self.added > 0 {
            parts.push(format!("{} added", self.added));
        }
        if self.changed > 0 {
            parts.push(format!("{} changed", self.changed));
        }
        if self.removed > 0 {
            parts.push(format!("{} removed", self.removed));
        }
        if parts.is_empty() {
            // entries unchanged — only sidecar (tags/notes) or formatting moved
            "niutero: update metadata".to_string()
        } else {
            format!("niutero: {}", parts.join(", "))
        }
    }
}

fn entry_diff(old: &str, new: &str) -> EntryDiff {
    fn by_key(s: &str) -> std::collections::HashMap<String, BibEntry> {
        entries(&parse(s))
            .map(|e| (e.citekey.clone(), e.clone()))
            .collect()
    }
    let (o, n) = (by_key(old), by_key(new));
    EntryDiff {
        added: n.keys().filter(|k| !o.contains_key(*k)).count(),
        removed: o.keys().filter(|k| !n.contains_key(*k)).count(),
        changed: n
            .iter()
            .filter(|(k, v)| o.get(*k).is_some_and(|ov| ov != *v))
            .count(),
    }
}

// ---------------------------------------------------------------- history

/// One commit in an entry's history — the stable shape `history --json` emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryCommit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

/// The commits that touched one entry, newest first.
///
/// `git log -L` numbers lines against the committed `HEAD`, so we locate the
/// entry's span in `HEAD`'s `references.bib` — not the working tree, whose
/// uncommitted edits could shift (or remove) the entry and make the range
/// wrong. Because the lookup is HEAD-driven, an entry deleted from the working
/// tree but still present in the last commit can still have its history shown;
/// the working tree is consulted only to give a precise error when the entry
/// isn't in HEAD (added-but-not-synced vs. no such entry at all).
pub fn history(v: &Vault, citekey: &str) -> Result<Vec<HistoryCommit>, String> {
    if !git::is_repo(&v.root) {
        return Err("not a git repository — run `niutero connect <url>` first".into());
    }
    let head = git::file_at_head(&v.root, "references.bib").ok_or_else(|| {
        "references.bib has no committed history yet — run `niutero sync` first".to_string()
    })?;
    let (start, end) = match entry_line_span(&head, citekey) {
        Some(span) => span,
        // Not in the last commit: distinguish "added locally, not synced yet"
        // from "no such entry" by consulting the working tree.
        None => {
            let exists = entries(&read_items(v)?).any(|e| e.citekey == citekey);
            return Err(if exists {
                format!("'{citekey}' isn't in the last commit yet — run `niutero sync` first")
            } else {
                format!("no entry with cite key '{citekey}'")
            });
        }
    };
    Ok(git::log_lines(&v.root, "references.bib", start, end)?
        .into_iter()
        .map(|c| HistoryCommit {
            hash: c.hash,
            author: c.author,
            date: c.date,
            subject: c.subject,
        })
        .collect())
}

// ------------------------------------------------------------------ rekey

/// One entry's cite-key change under [`rekey_preview`] / [`rekey_apply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rekey {
    pub citekey: String,
    pub new_key: String,
    /// A disambiguating suffix was appended because the generated key collided.
    pub disambiguated: bool,
}

/// Preview re-keying: the cite-key changes the library's pattern would make,
/// without touching disk. `pattern` overrides the vault's configured pattern.
/// Validates the proposed keys, so a pattern that would produce an illegal key
/// fails here exactly as it would on `--write` (preview and apply agree).
pub fn rekey_preview(v: &Vault, pattern: Option<&str>) -> Result<Vec<Rekey>, String> {
    let items = read_items(v)?;
    let changes = plan_rekey(&items, &resolve_pattern(v, pattern));
    validate_rekey(&items, &changes)?;
    Ok(changes)
}

/// Apply re-keying: rewrite each entry's cite key per the pattern and migrate
/// the sidecar (tags/notes/status/stars are keyed by cite key, so they must
/// follow the rename). Returns the changes applied (entries whose key changed).
pub fn rekey_apply(v: &mut Vault, pattern: Option<&str>) -> Result<Vec<Rekey>, String> {
    let _lock = lock_vault(v)?;
    let mut items = read_items(v)?;
    let changes = plan_rekey(&items, &resolve_pattern(v, pattern));
    if changes.is_empty() {
        return Ok(changes);
    }
    validate_rekey(&items, &changes)?; // before any write
    let original = items.clone(); // for rollback if the sidecar write fails
    let renames: std::collections::HashMap<&str, &str> = changes
        .iter()
        .map(|c| (c.citekey.as_str(), c.new_key.as_str()))
        .collect();
    for it in &mut items {
        if let BibItem::Entry(e) = it {
            if let Some(new) = renames.get(e.citekey.as_str()) {
                e.citekey = (*new).to_string();
            }
        }
    }
    write_items(v, &items)?;

    // Migrate sidecar metadata to the new keys. This is pure in-memory work, so
    // it can't fail; the only fallible step left is persisting it. If that
    // fails, roll `references.bib` back to its pre-rekey state so the on-disk
    // `.bib` and sidecar never disagree about which keys exist.
    let mut migrated: niutero_vault::Meta = std::collections::BTreeMap::new();
    for c in &changes {
        if let Some(m) = v.meta.remove(&c.citekey) {
            migrated.insert(c.new_key.clone(), m);
        }
    }
    // Fold in any leftover (stale) meta — entries with no matching `.bib` entry.
    // On a key clash a live entry's migrated meta wins; the stale value is dropped.
    for (k, m) in std::mem::take(&mut v.meta) {
        migrated.entry(k).or_insert(m);
    }
    v.meta = migrated;
    if let Err(e) = save_sidecar(v) {
        let _ = write_items(v, &original);
        return Err(e);
    }
    Ok(changes)
}

/// Validate each renamed entry under its new cite key, so an illegal generated
/// key (e.g. a custom pattern whose literals inject a forbidden character) is
/// caught before any write.
fn validate_rekey(items: &[BibItem], changes: &[Rekey]) -> Result<(), String> {
    let renames: std::collections::HashMap<&str, &str> = changes
        .iter()
        .map(|c| (c.citekey.as_str(), c.new_key.as_str()))
        .collect();
    for e in entries(items) {
        if let Some(new) = renames.get(e.citekey.as_str()) {
            let mut renamed = e.clone();
            renamed.citekey = (*new).to_string();
            renamed.validate()?;
        }
    }
    Ok(())
}

/// Plan the new key for every entry, reporting only those that change. Entries
/// whose pattern output already equals their current key keep it (reserved
/// first), so re-keying churns only the keys that actually need to move;
/// remaining collisions get a deterministic `a`/`b`/… suffix in document order.
fn plan_rekey(items: &[BibItem], pattern: &KeyPattern) -> Vec<Rekey> {
    let es: Vec<&BibEntry> = entries(items).collect();
    let bases: Vec<String> = es
        .iter()
        .map(|e| {
            let b = pattern.render(e);
            if b.trim().is_empty() {
                e.citekey.clone() // empty pattern output: leave the key alone
            } else {
                b
            }
        })
        .collect();
    let mut taken: std::collections::HashSet<String> = es
        .iter()
        .zip(&bases)
        .filter(|(e, base)| **base == e.citekey)
        .map(|(e, _)| e.citekey.clone())
        .collect();
    let mut out = Vec::new();
    for (e, base) in es.iter().zip(&bases) {
        if *base == e.citekey {
            continue; // unchanged; already reserved above
        }
        let (new_key, disambiguated) = next_free_key(base, &taken);
        taken.insert(new_key.clone()); // reserve even on a self-rename below
                                       // The disambiguating suffix can resolve back to this entry's own
                                       // key (e.g. it already carries the suffix); that's a no-op, so
                                       // re-keying stays idempotent.
        if new_key != e.citekey {
            out.push(Rekey {
                citekey: e.citekey.clone(),
                new_key,
                disambiguated,
            });
        }
    }
    out
}

fn resolve_pattern(v: &Vault, override_: Option<&str>) -> KeyPattern {
    let s = override_
        .map(str::to_string)
        .or_else(|| v.config.citekey_pattern.clone())
        .unwrap_or_else(|| KeyPattern::DEFAULT.to_string());
    KeyPattern::parse(&s)
}

// ---------------------------------------------------------------- analyze

/// One library-health check: a class of issue plus the entries that have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub hint: String,
    pub keys: Vec<String>,
}

/// An offline scan of the library's overall hygiene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysisReport {
    pub total: usize,
    pub checks: Vec<Check>,
}

/// Scan the library for hygiene issues, entirely offline. Reports, per check,
/// the entries that fail it. Online checks (e.g. "arXiv mislabeled" needs a DOI
/// lookup) and duplicate-merge are intentionally out of scope here.
pub fn analyze(v: &Vault) -> Result<AnalysisReport, String> {
    let items = read_items(v)?;
    let cfg = NormConfig::load(&v.niutero_dir());
    let es: Vec<&BibEntry> = entries(&items).collect();

    let offline: Vec<String> = norm_changes(&items, &cfg)
        .1
        .into_iter()
        .map(|c| c.citekey)
        .collect();
    let odd_titles = keys_where(&es, |e| is_odd_title(e.get("title")));
    let missing_url = keys_where(&es, |e| blank(e.get("url")) && blank(e.get("doi")));
    let missing_year = keys_where(&es, |e| blank(e.get("year")));
    let inconsistent_venues = inconsistent_venue_keys(&es);
    let owned: Vec<BibEntry> = es.iter().map(|&e| e.clone()).collect();
    let duplicates: Vec<String> = dedup::duplicate_groups(&owned)
        .into_iter()
        .flatten()
        .collect();

    Ok(AnalysisReport {
        total: es.len(),
        checks: vec![
            Check {
                id: "offline".into(),
                label: "Offline-changeable".into(),
                hint: "A local cleanup pass (normalize) would rewrite these".into(),
                keys: offline,
            },
            Check {
                id: "titles".into(),
                label: "Odd titles".into(),
                hint: "ALL-CAPS, missing, or very short titles".into(),
                keys: odd_titles,
            },
            Check {
                id: "venues".into(),
                label: "Inconsistent venues".into(),
                hint: "Same venue spelled several ways (casing / punctuation)".into(),
                keys: inconsistent_venues,
            },
            Check {
                id: "url".into(),
                label: "Missing URL".into(),
                hint: "No url or doi to resolve the entry".into(),
                keys: missing_url,
            },
            Check {
                id: "year".into(),
                label: "Missing year".into(),
                hint: "No publication year set".into(),
                keys: missing_year,
            },
            Check {
                id: "dupes".into(),
                label: "Likely duplicates".into(),
                hint: "Same first author + year + title (see `dedupe`)".into(),
                keys: duplicates,
            },
        ],
    })
}

// ----------------------------------------------------------------- dedupe

/// One duplicate cluster — cite keys that look like the same work, ordered with
/// the entry [`dedupe_merge`] would keep (the richest, most-fields one) first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DupGroup {
    pub citekeys: Vec<String>,
}

/// One merge that [`dedupe_merge`] performed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DupMerge {
    pub kept: String,
    pub dropped: Vec<String>,
}

/// Likely-duplicate clusters, without changing anything.
pub fn dedupe_preview(v: &Vault) -> Result<Vec<DupGroup>, String> {
    let items = read_items(v)?;
    let es: Vec<BibEntry> = entries(&items).cloned().collect();
    Ok(dedup::duplicate_groups(&es)
        .into_iter()
        .map(|g| DupGroup {
            citekeys: order_primary_first(&es, g),
        })
        .collect())
}

/// Merge each duplicate cluster into its primary: union any fields the primary
/// is missing, fold the dropped entries' sidecar in (tags unioned, highest
/// stars / status kept, notes appended), then remove the duplicates and their
/// metadata. Returns the merges performed.
pub fn dedupe_merge(v: &mut Vault) -> Result<Vec<DupMerge>, String> {
    let _lock = lock_vault(v)?;
    let mut items = read_items(v)?;
    let snapshot: Vec<BibEntry> = entries(&items).cloned().collect();
    let groups = dedup::duplicate_groups(&snapshot);
    if groups.is_empty() {
        return Ok(Vec::new());
    }

    let mut merges = Vec::new();
    let mut to_remove: std::collections::HashSet<String> = std::collections::HashSet::new();
    for group in groups {
        let ordered = order_primary_first(&snapshot, group);
        let (primary, others) = ordered.split_first().expect("group has ≥2 keys");

        // Union into the primary entry any field it is missing (primary wins).
        if let Some(BibItem::Entry(p)) = items
            .iter_mut()
            .find(|it| matches!(it, BibItem::Entry(e) if &e.citekey == primary))
        {
            for key in others {
                if let Some(donor) = snapshot.iter().find(|e| &e.citekey == key) {
                    for (name, value) in &donor.fields {
                        // A blank placeholder (e.g. `doi = {}`) on the primary
                        // counts as missing, so a donor's real value fills it in.
                        let primary_blank = p.get(name).is_none_or(|v| v.trim().is_empty());
                        if primary_blank && !value.trim().is_empty() {
                            p.set(name, value);
                        }
                    }
                }
            }
            p.validate()?;
        }

        merge_dup_sidecar(v, primary, others);
        to_remove.extend(others.iter().cloned());
        merges.push(DupMerge {
            kept: primary.clone(),
            dropped: others.to_vec(),
        });
    }

    items.retain(|it| !matches!(it, BibItem::Entry(e) if to_remove.contains(&e.citekey)));
    write_items(v, &items)?;
    save_sidecar(v)?;
    Ok(merges)
}

/// Order a duplicate group with the richest entry first (ties keep document
/// order) so the primary is the most complete one. "Richest" counts only
/// non-blank fields, so an entry padded with empty placeholders (`doi = {}`)
/// isn't mistaken for the fuller one.
fn order_primary_first(es: &[BibEntry], mut group: Vec<String>) -> Vec<String> {
    group.sort_by_key(|k| {
        let filled = es.iter().find(|e| &e.citekey == k).map_or(0, |e| {
            e.fields.values().filter(|v| !v.trim().is_empty()).count()
        });
        std::cmp::Reverse(filled)
    });
    group
}

/// Fold the `dropped` entries' sidecar metadata into the `primary`'s, removing
/// the dropped entries' metadata. Tags are unioned; the highest star rating and
/// reading status win; distinct notes are appended.
fn merge_dup_sidecar(v: &mut Vault, primary: &str, dropped: &[String]) {
    let donors: Vec<EntryMeta> = dropped.iter().filter_map(|k| v.meta.remove(k)).collect();
    if donors.is_empty() && !v.meta.contains_key(primary) {
        return;
    }
    let meta = v.meta.entry(primary.to_string()).or_default();
    for d in donors {
        for tag in d.tags {
            if !meta.tags.contains(&tag) {
                meta.tags.push(tag);
            }
        }
        if meta.note.is_empty() {
            meta.note = d.note;
        } else if !d.note.is_empty() && !meta.note.contains(&d.note) {
            meta.note.push('\n');
            meta.note.push_str(&d.note);
        }
        meta.stars = meta.stars.max(d.stars);
        if status_rank(d.status) > status_rank(meta.status) {
            meta.status = d.status;
        }
    }
    meta.tags.sort();
    meta.tags.dedup();
    prune_meta(v, primary);
}

fn status_rank(s: Option<Status>) -> u8 {
    match s {
        Some(Status::Done) => 2,
        Some(Status::Reading) => 1,
        Some(Status::Unread) | None => 0,
    }
}

fn blank(field: Option<&str>) -> bool {
    field.unwrap_or("").trim().is_empty()
}

fn keys_where(es: &[&BibEntry], pred: impl Fn(&BibEntry) -> bool) -> Vec<String> {
    es.iter()
        .filter(|e| pred(e))
        .map(|e| e.citekey.clone())
        .collect()
}

/// A title is "odd" if it is missing/blank, has fewer than three alphanumerics
/// (a sign of truncation), or is a multi-word ALL-CAPS shout. The multi-word
/// guard avoids flagging legitimate single-token acronyms (`GAN`, `GPT-3`) and
/// brace-protected proper nouns (`{{BERT}}`), which the normalizer itself emits.
fn is_odd_title(title: Option<&str>) -> bool {
    let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) else {
        return true;
    };
    // Strip BibTeX brace protection so `{{BERT}}` reads as `BERT`.
    let plain: String = t.chars().filter(|&c| c != '{' && c != '}').collect();
    if plain.chars().filter(|c| c.is_alphanumeric()).count() < 3 {
        return true; // too short / truncated
    }
    let words_with_letters = plain
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .count();
    let letters: Vec<char> = plain.chars().filter(|c| c.is_alphabetic()).collect();
    // ALL-CAPS only counts as a "shout" across two or more words.
    words_with_letters >= 2 && !letters.is_empty() && letters.iter().all(|c| c.is_uppercase())
}

/// Entries whose venue (booktitle / journal / venue) shares a normalized form
/// with a *different* spelling elsewhere in the library.
fn inconsistent_venue_keys(es: &[&BibEntry]) -> Vec<String> {
    fn venue(e: &BibEntry) -> Option<&str> {
        ["booktitle", "journal", "venue"]
            .iter()
            .find_map(|f| e.get(f))
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
    fn normalized(s: &str) -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    }
    let mut groups: std::collections::BTreeMap<
        String,
        (std::collections::BTreeSet<String>, Vec<String>),
    > = std::collections::BTreeMap::new();
    for e in es {
        if let Some(v) = venue(e) {
            let g = groups.entry(normalized(v)).or_default();
            g.0.insert(v.to_string());
            g.1.push(e.citekey.clone());
        }
    }
    groups
        .into_values()
        .filter(|(spellings, _)| spellings.len() > 1)
        .flat_map(|(_, keys)| keys)
        .collect()
}

// -------------------------------------------------------------- normalize

/// A single field-level change normalization would make (`from`/`to` are
/// `None` when the field is absent on that side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// One entry that offline normalization would change, with human-readable notes
/// and the structured field-level diff (for a UI / `--json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormChange {
    pub citekey: String,
    pub notes: Vec<String>,
    pub diffs: Vec<FieldChange>,
}

/// Preview offline normalization — compute what would change without writing.
/// `profile` selects a `[profiles.<name>]` from `norm.toml` (None = base config).
pub fn normalize_preview(v: &Vault, profile: Option<&str>) -> Result<Vec<NormChange>, String> {
    let items = read_items(v)?;
    let cfg = NormConfig::resolve(&v.niutero_dir(), profile)?;
    Ok(norm_changes(&items, &cfg).1)
}

/// Apply offline normalization, writing the result (only if something changed).
/// Returns the changes that were applied. `profile` as in [`normalize_preview`].
pub fn normalize_apply(v: &Vault, profile: Option<&str>) -> Result<Vec<NormChange>, String> {
    let _lock = lock_vault(v)?;
    let items = read_items(v)?;
    let cfg = NormConfig::resolve(&v.niutero_dir(), profile)?;
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
                let diffs = entry_field_diff(e, &normalized);
                // `notes` and `diffs` are non-empty together today; the `||` is
                // defensive, so a future rule that mutates a field without a note
                // is still recorded (and surfaced in the structured diff).
                if !notes.is_empty() || !diffs.is_empty() {
                    changes.push(NormChange {
                        citekey: e.citekey.clone(),
                        notes,
                        diffs,
                    });
                }
                out.push(BibItem::Entry(normalized));
            }
            BibItem::Verbatim(s) => out.push(BibItem::Verbatim(s.clone())),
        }
    }
    (out, changes)
}

/// The field-level delta between an entry and its normalized form: the entry
/// type plus each added / removed / changed field, in normalized-then-removed
/// order.
fn entry_field_diff(orig: &BibEntry, norm: &BibEntry) -> Vec<FieldChange> {
    let mut diffs = Vec::new();
    if orig.entry_type() != norm.entry_type() {
        diffs.push(FieldChange {
            field: "entrytype".into(),
            from: Some(orig.entry_type().to_string()),
            to: Some(norm.entry_type().to_string()),
        });
    }
    for (k, nv) in &norm.fields {
        match orig.fields.get(k) {
            Some(ov) if ov != nv => diffs.push(FieldChange {
                field: k.clone(),
                from: Some(ov.clone()),
                to: Some(nv.clone()),
            }),
            Some(_) => {}
            None => diffs.push(FieldChange {
                field: k.clone(),
                from: None,
                to: Some(nv.clone()),
            }),
        }
    }
    for (k, ov) in &orig.fields {
        if !norm.fields.contains_key(k) {
            diffs.push(FieldChange {
                field: k.clone(),
                from: Some(ov.clone()),
                to: None,
            });
        }
    }
    diffs
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

/// Generate a unique cite key for `entry` from the library's pattern, avoiding
/// keys already present in `items`. Falls back to `ref` when the pattern yields
/// nothing (the entry has no author / year / title to build from).
fn generate_citekey(v: &Vault, entry: &BibEntry, items: &[BibItem]) -> String {
    let base = resolve_pattern(v, None).render(entry);
    let base = if base.trim().is_empty() {
        "ref".to_string()
    } else {
        base
    };
    let taken: std::collections::HashSet<String> =
        entries(items).map(|e| e.citekey.clone()).collect();
    next_free_key(&base, &taken).0
}

/// `base` if free, else `base` + the first free letter suffix (`a`, `b`, …,
/// `z`, `aa`, …). The bool reports whether a suffix was needed.
fn next_free_key(base: &str, taken: &std::collections::HashSet<String>) -> (String, bool) {
    if !taken.contains(base) {
        return (base.to_string(), false);
    }
    (1..)
        .map(|n| format!("{base}{}", letter_suffix(n)))
        .find(|k| !taken.contains(k))
        .map(|k| (k, true))
        .expect("infinite range yields an unused key")
}

/// `1 → "a"`, `26 → "z"`, `27 → "aa"`, … (spreadsheet-column lettering).
fn letter_suffix(mut n: usize) -> String {
    let mut chars = Vec::new();
    while n > 0 {
        n -= 1;
        chars.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    chars.iter().rev().collect()
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

/// Take the vault's exclusive lock for a mutating operation; held until the
/// returned guard drops. Serializes concurrent `niutero` processes so a
/// read-modify-write race can't lose an update.
fn lock_vault(v: &Vault) -> Result<niutero_vault::VaultLock, String> {
    v.lock().map_err(|e| format!("lock library: {e}"))
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
    let meta = v.meta.get(&entry.citekey);
    EntryView {
        citekey: entry.citekey.clone(),
        entry_type: entry.entry_type().to_string(),
        fields: entry.fields.clone(),
        tags: meta.map(|m| m.tags.clone()).unwrap_or_default(),
        note: meta.map(|m| m.note.clone()).unwrap_or_default(),
        status: meta
            .and_then(|m| m.status)
            .unwrap_or(Status::Unread)
            .as_str()
            .to_string(),
        stars: meta.and_then(|m| m.stars),
        added: meta.and_then(|m| m.added.clone()),
    }
}

/// Build the query facets (tags / status / stars) for an entry from its sidecar.
fn facets_of<'a>(v: &'a Vault, citekey: &str) -> Facets<'a> {
    let meta = v.meta.get(citekey);
    Facets {
        tags: meta.map(|m| m.tags.as_slice()).unwrap_or(&[]),
        status: meta.and_then(|m| m.status).map(|s| s.as_str()),
        stars: meta.and_then(|m| m.stars),
    }
}

/// Write a vault README explaining the layout, only if one isn't there.
fn write_readme_if_absent(v: &Vault) -> std::io::Result<()> {
    let path = v.root.join("README.md");
    if path.exists() {
        return Ok(());
    }
    let readme = format!(
        "# {}\n\nA niutero citation library. `references.bib` is the portable \
source of truth — hand it to any tool. niutero's private data (tags, notes, \
saved views, config) lives in `.niutero/` and never touches the `.bib`.\n\n\
Edit with the `niutero` CLI, or edit `references.bib` directly; the `.niutero/` \
sidecar is safe to commit alongside the library.\n",
        v.config.name
    );
    std::fs::write(path, readme)
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
    fn mutations_respect_the_vault_lock() {
        let (_d, v) = vault();
        // While the lock is held elsewhere, a mutating op refuses to proceed…
        let guard = v.lock().unwrap();
        assert!(add(&v, fields("misc", "k", &[]))
            .unwrap_err()
            .contains("lock library"));
        // …and a read-only op is unaffected.
        assert!(list(&v, Filter::All).is_ok());
        // Once released, mutations work again.
        drop(guard);
        assert!(add(&v, fields("misc", "k", &[])).is_ok());
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
                ..Default::default()
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
            SyncStatus::Synced {
                committed: true,
                merged: false
            }
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

        // A and B change the same field differently; B's sync can't auto-merge.
        edit(&a, "k", &["title=A2".into()], &[], None).unwrap();
        sync(&a, None).unwrap();
        edit(&b, "k", &["title=B2".into()], &[], None).unwrap();
        assert_eq!(sync(&b, None).unwrap(), SyncStatus::Conflict);
        // The aborted merge leaves a clean tree, so B can keep working.
        assert!(!niutero_sync::conflicted_paths(&b.root)
            .iter()
            .any(|p| p == "references.bib"));
    }

    #[test]
    fn sync_auto_resolves_a_field_level_merge() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        // A: one entry with field `a`, pushed.
        let (_da, a) = vault();
        connect(&a, bare).unwrap();
        git_identity(&a.root);
        add(&a, fields("misc", "k", &["a=1"])).unwrap();
        sync(&a, None).unwrap();

        // B clones it.
        let dstb = tempfile::tempdir().unwrap();
        let b_path = dstb.path().join("b");
        git_clone(bare, &b_path);
        git_identity(&b_path);
        let b = open(&b_path).unwrap();

        // A adds field `b`; B adds field `c` — different fields of the SAME entry.
        // git's line merge conflicts (both insert after `a`), but the structured
        // entry merge reconciles them.
        edit(&a, "k", &["b=2".into()], &[], None).unwrap();
        sync(&a, None).unwrap();
        edit(&b, "k", &["c=3".into()], &[], None).unwrap();

        assert_eq!(
            sync(&b, None).unwrap(),
            SyncStatus::Synced {
                committed: true,
                merged: true
            }
        );
        // B's entry now carries all three fields, with no conflict left behind.
        let view = show(&b, "k").unwrap();
        assert_eq!(view.fields.get("a").map(String::as_str), Some("1"));
        assert_eq!(view.fields.get("b").map(String::as_str), Some("2"));
        assert_eq!(view.fields.get("c").map(String::as_str), Some("3"));
        assert!(niutero_sync::conflicted_paths(&b.root).is_empty());

        // A pulls B's merge cleanly — the libraries converge.
        assert!(matches!(sync(&a, None).unwrap(), SyncStatus::Synced { .. }));
        assert_eq!(show(&a, "k").unwrap().fields.len(), 3);
    }

    /// Set up a bare remote with `a`'s content pushed, and `b` a fresh clone of
    /// it (both with a committer identity). Returns the temp dirs to keep alive.
    fn synced_clones(
        seed: impl FnOnce(&Vault),
    ) -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Vault,
        tempfile::TempDir,
        Vault,
    ) {
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();
        let (da, a) = vault();
        connect(&a, bare).unwrap();
        git_identity(&a.root);
        seed(&a);
        sync(&a, None).unwrap();
        let db = tempfile::tempdir().unwrap();
        let b_path = db.path().join("b");
        git_clone(bare, &b_path);
        git_identity(&b_path);
        let b = open(&b_path).unwrap();
        (remote, da, a, db, b)
    }

    #[test]
    fn sync_aborts_on_modify_delete_instead_of_dropping_entries() {
        if !niutero_sync::git_available() {
            return;
        }
        let (_r, _da, a, _db, b) = synced_clones(|a| {
            add(a, fields("misc", "k", &["title=K"])).unwrap();
            add(a, fields("misc", "j", &["title=J"])).unwrap();
        });
        // A adds a third entry; B deletes references.bib wholesale.
        add(&a, fields("misc", "n", &["title=N"])).unwrap();
        sync(&a, None).unwrap();
        std::fs::remove_file(b.bib_path()).unwrap();

        // The modify/delete must NOT auto-resolve (that would silently drop k/j).
        assert_eq!(sync(&b, None).unwrap(), SyncStatus::Conflict);
    }

    #[test]
    fn sync_aborts_when_a_string_macro_conflicts() {
        if !niutero_sync::git_available() {
            return;
        }
        let base = "@string{acl = {ACL}}\n\n@misc{k,\n  title = {T}\n}\n";
        let (_r, _da, a, _db, b) = synced_clones(|a| {
            std::fs::write(a.bib_path(), base).unwrap();
        });
        // Both sides edit the SAME @string macro differently — a verbatim conflict
        // git can't resolve; we must abort, never commit a duplicated macro.
        std::fs::write(a.bib_path(), base.replace("{ACL}", "{Assoc CL}")).unwrap();
        sync(&a, None).unwrap();
        std::fs::write(b.bib_path(), base.replace("{ACL}", "{ACL Org}")).unwrap();
        assert_eq!(sync(&b, None).unwrap(), SyncStatus::Conflict);
    }

    #[test]
    fn sync_aborts_on_a_sidecar_conflict() {
        if !niutero_sync::git_available() {
            return;
        }
        let (_r, _da, mut a, _db, b) = synced_clones(|a| {
            add(a, fields("misc", "k", &["title=T"])).unwrap();
        });
        let mut b = b;
        // Both tag the same entry differently → `.niutero/meta.json` conflicts.
        // That's not references.bib, so the merge must abort, untouched.
        set_tags(&mut a, "k", &["topics:ours".into()], &[]).unwrap();
        sync(&a, None).unwrap();
        set_tags(&mut b, "k", &["topics:theirs".into()], &[]).unwrap();
        assert_eq!(sync(&b, None).unwrap(), SyncStatus::Conflict);
    }

    #[test]
    fn auto_merge_preserves_a_string_blocks() {
        if !niutero_sync::git_available() {
            return;
        }
        let base = "@string{acl = {ACL}}\n\n@misc{k,\n  a = {1}\n}\n";
        let (_r, _da, a, _db, b) = synced_clones(|a| {
            std::fs::write(a.bib_path(), base).unwrap();
        });
        // Disjoint field edits to `k` (a line conflict), with the @string left
        // identical on both sides → auto-merges, and the @string must survive once.
        edit(&a, "k", &["b=2".into()], &[], None).unwrap();
        sync(&a, None).unwrap();
        edit(&b, "k", &["c=3".into()], &[], None).unwrap();
        assert_eq!(
            sync(&b, None).unwrap(),
            SyncStatus::Synced {
                committed: true,
                merged: true
            }
        );
        let bib = std::fs::read_to_string(b.bib_path()).unwrap();
        assert_eq!(bib.matches("@string{acl = {ACL}}").count(), 1, "got: {bib}");
        // the merged file is canonical: re-parsing and re-serializing is a no-op.
        assert_eq!(niutero_bib::to_bibtex(&parse(&bib)), bib);
        let view = show(&b, "k").unwrap();
        assert_eq!(view.fields.get("b").map(String::as_str), Some("2"));
        assert_eq!(view.fields.get("c").map(String::as_str), Some("3"));
    }

    #[test]
    fn history_lists_an_entrys_commits_newest_first() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, v) = vault();
        connect(&v, bare).unwrap();
        git_identity(&v.root);
        add(&v, fields("misc", "k", &["title=One"])).unwrap();
        sync(&v, None).unwrap(); // commit 1: "niutero: initial import"
        edit(&v, "k", &["title=Two".into()], &[], None).unwrap();
        sync(&v, None).unwrap(); // commit 2: "niutero: 1 changed"

        let h = history(&v, "k").unwrap();
        assert_eq!(h.len(), 2);
        assert!(h[0].subject.contains("1 changed"), "newest: {:?}", h[0]);
        assert!(
            h[1].subject.contains("initial import"),
            "oldest: {:?}",
            h[1]
        );
        assert!(!h[0].hash.is_empty() && !h[0].date.is_empty());
    }

    #[test]
    fn history_errors_when_not_a_repo() {
        let (_d, v) = vault();
        add(&v, fields("misc", "k", &[])).unwrap();
        // Without git, the missing repo is the actionable problem — regardless of
        // whether the queried key exists.
        assert!(history(&v, "k")
            .unwrap_err()
            .contains("not a git repository"));
        assert!(history(&v, "ghost")
            .unwrap_err()
            .contains("not a git repository"));
    }

    #[test]
    fn history_errors_when_no_commits_yet() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, v) = vault();
        connect(&v, bare).unwrap(); // inits git, but commits nothing
        git_identity(&v.root);
        add(&v, fields("misc", "k", &[])).unwrap();
        // Connected, but never synced: a repo with no HEAD.
        assert!(history(&v, "k")
            .unwrap_err()
            .contains("no committed history yet"));
    }

    #[test]
    fn history_for_an_uncommitted_entry_is_actionable() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, v) = vault();
        connect(&v, bare).unwrap();
        git_identity(&v.root);
        add(&v, fields("misc", "committed", &["title=A"])).unwrap();
        sync(&v, None).unwrap();
        // Added locally but not synced: in the working tree, not in HEAD.
        add(&v, fields("misc", "fresh", &["title=B"])).unwrap();
        assert!(history(&v, "fresh")
            .unwrap_err()
            .contains("isn't in the last commit"));
    }

    #[test]
    fn history_traces_only_the_queried_entry() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, v) = vault();
        connect(&v, bare).unwrap();
        git_identity(&v.root);
        add(&v, fields("misc", "a", &["title=A"])).unwrap();
        add(&v, fields("misc", "b", &["title=B"])).unwrap();
        sync(&v, None).unwrap(); // commit 1: both entries (a above b)
        edit(&v, "b", &["title=B2".into()], &[], None).unwrap();
        sync(&v, None).unwrap(); // commit 2: only b's lines changed

        // `a`'s lines were only ever touched by the first commit.
        let ha = history(&v, "a").unwrap();
        assert_eq!(ha.len(), 1, "a should not pick up b's commit: {ha:?}");
        assert!(ha[0].subject.contains("initial import"));
        // `b` was added then changed — both commits touched its lines.
        let hb = history(&v, "b").unwrap();
        assert_eq!(hb.len(), 2, "b: {hb:?}");

        // A key in neither HEAD nor the working tree is a plain "no entry".
        assert!(history(&v, "ghost")
            .unwrap_err()
            .contains("no entry with cite key 'ghost'"));
    }

    #[test]
    fn history_follows_a_moved_entry_using_the_head_span() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, v) = vault();
        connect(&v, bare).unwrap();
        git_identity(&v.root);
        let write = |bib: &str| std::fs::write(v.bib_path(), bib).unwrap();

        // c1: only `a`, on lines 1-3.
        write("@misc{a,\n  t = {A}\n}\n");
        sync(&v, None).unwrap();
        // c2: insert `z` before `a`, moving `a` to lines 5-7 (its content unchanged).
        write("@misc{z,\n  t = {Z}\n}\n\n@misc{a,\n  t = {A}\n}\n");
        sync(&v, None).unwrap();
        // c3: change `a`, now living at lines 5-7.
        write("@misc{z,\n  t = {Z}\n}\n\n@misc{a,\n  t = {A2}\n}\n");
        sync(&v, None).unwrap();
        // Uncommitted: shift `a` again so the WORKING-TREE span (9-11) differs from
        // HEAD's (5-7) — and the working tree is now longer than HEAD. A working-
        // tree-based lookup would feed git an out-of-range line range and fail;
        // the HEAD-based lookup must still succeed.
        write("@misc{y,\n  t = {Y}\n}\n\n@misc{z,\n  t = {Z}\n}\n\n@misc{a,\n  t = {A2}\n}\n");

        let h =
            history(&v, "a").expect("history must use the HEAD span, not the longer working tree");
        let subjects: Vec<&str> = h.iter().map(|c| c.subject.as_str()).collect();
        // git follows `a` back across its move, so its introduction (c1) and its
        // edit (c3) both appear, proving the right (current HEAD) span was traced.
        assert!(
            subjects.iter().any(|s| s.contains("initial import")),
            "should trace `a` back to its introduction: {subjects:?}"
        );
        assert!(
            subjects.iter().any(|s| s.contains("1 changed")),
            "should include the edit to `a`: {subjects:?}"
        );
    }

    #[test]
    fn history_works_for_an_entry_deleted_from_the_working_tree() {
        if !niutero_sync::git_available() {
            return;
        }
        let remote = tempfile::tempdir().unwrap();
        run_git(remote.path(), &["init", "--bare"]);
        let bare = remote.path().to_str().unwrap();

        let (_d, mut v) = vault();
        connect(&v, bare).unwrap();
        git_identity(&v.root);
        add(&v, fields("misc", "k", &["title=A"])).unwrap();
        sync(&v, None).unwrap(); // k is now in HEAD
        rm(&mut v, "k").unwrap(); // removed locally, but not yet committed
        assert!(
            show(&v, "k").is_err(),
            "k should be gone from the working tree"
        );
        // history is HEAD-driven, so a locally-deleted entry's past is still viewable.
        let h = history(&v, "k").unwrap();
        assert_eq!(h.len(), 1);
        assert!(h[0].subject.contains("initial import"));
    }

    #[test]
    fn entry_diff_counts_add_change_remove() {
        let old = "@misc{a, title={A}}\n@misc{b, title={B}}\n";
        let new = "@misc{a, title={A2}}\n@misc{c, title={C}}\n"; // a changed, b gone, c new
        let d = entry_diff(old, new);
        assert_eq!((d.added, d.changed, d.removed), (1, 1, 1));
        assert_eq!(d.message(), "niutero: 1 added, 1 changed, 1 removed");
    }

    #[test]
    fn entry_diff_message_when_only_metadata_moved() {
        let same = "@misc{a, title={A}}\n";
        assert_eq!(entry_diff(same, same).message(), "niutero: update metadata");
    }

    #[test]
    fn connect_writes_gitattributes_and_pins_endings() {
        if !niutero_sync::git_available() {
            return;
        }
        let (_d, v) = vault();
        connect(&v, "https://example.com/r.git").unwrap();
        let ga = std::fs::read_to_string(v.root.join(".gitattributes")).unwrap();
        assert!(ga.contains("eol=lf"), "got: {ga}");
        assert!(niutero_sync::is_repo(&v.root));
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
        let changes = normalize_preview(&v, None).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].citekey, "k");
        assert_eq!(std::fs::read_to_string(v.bib_path()).unwrap(), before);

        // apply writes; the noise field is gone and whitespace tidied
        assert_eq!(normalize_apply(&v, None).unwrap().len(), 1);
        let view = show(&v, "k").unwrap();
        assert!(view.fields.get("abstract").is_none());
        // capitalized title words are {{}}-protected (bib_fixer behavior)
        assert_eq!(
            view.fields.get("title").map(String::as_str),
            Some("{{A}} {{B}}")
        );

        // idempotent: nothing left to change
        assert!(normalize_preview(&v, None).unwrap().is_empty());
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
            "keep_fields = [\"title\", \"abstract\"]\nprotect_title_caps = false\n\
             conference_acronyms = false\ndoi_to_url = false\ntidy_whitespace = false\n",
        )
        .unwrap();
        assert!(normalize_preview(&v, None).unwrap().is_empty());
    }

    #[test]
    fn normalize_profile_selects_an_alternative_config() {
        let (_d, v) = vault();
        add(
            &v,
            AddSource::Bibtex("@article{k, title={Deep Learning}, abstract={x}}".into()),
        )
        .unwrap();
        // Base config: keep abstract, everything off → nothing changes. The
        // `aggressive` profile sets only protect_title_caps, so the rest fall
        // back to the built-in defaults (default keep-list drops abstract).
        std::fs::write(
            v.niutero_dir().join("norm.toml"),
            "keep_fields = [\"title\", \"abstract\"]\nprotect_title_caps = false\n\
             conference_acronyms = false\ndoi_to_url = false\ntidy_whitespace = false\n\n\
             [profiles.aggressive]\nprotect_title_caps = true\n",
        )
        .unwrap();

        assert!(normalize_preview(&v, None).unwrap().is_empty());
        assert_eq!(normalize_preview(&v, Some("aggressive")).unwrap().len(), 1);
        assert!(normalize_preview(&v, Some("nope"))
            .unwrap_err()
            .contains("no normalize profile 'nope'"));
    }

    #[test]
    fn init_scaffolds_readme_and_norm_toml() {
        let (d, _v) = vault();
        assert!(d.path().join("README.md").exists());
        assert!(d.path().join(".niutero").join("norm.toml").exists());
    }

    #[test]
    fn cite_formats_existing_and_errors_on_missing() {
        let (_d, v) = vault();
        add(&v, fields("misc", "k", &[])).unwrap();
        assert_eq!(cite(&v, "k").unwrap(), "\\cite{k}");
        assert!(cite(&v, "nope").unwrap_err().contains("no entry"));
    }

    // ------------------------------------------------- citekey pattern / rekey

    #[test]
    fn add_generates_a_key_when_none_is_given() {
        let (_d, v) = vault();
        let keys = add(
            &v,
            fields(
                "article",
                "", // no cite key
                &[
                    "author=Vaswani, Ashish",
                    "year=2017",
                    "title=Attention Is All You Need",
                ],
            ),
        )
        .unwrap();
        assert_eq!(keys, vec!["vaswani2017attentionIsAll".to_string()]);
        assert!(show(&v, "vaswani2017attentionIsAll").is_ok());
    }

    #[test]
    fn add_autokey_disambiguates_against_existing() {
        let (_d, v) = vault();
        let mk = || {
            fields(
                "article",
                "",
                &["author=Gao, Leo", "year=2025", "title=Scaling Sparse"],
            )
        };
        let k1 = add(&v, mk()).unwrap();
        let k2 = add(&v, mk()).unwrap();
        assert_eq!(k1[0], "gao2025scalingSparse");
        assert_eq!(k2[0], "gao2025scalingSparsea"); // letter suffix on collision
    }

    #[test]
    fn rekey_applies_and_migrates_the_sidecar() {
        let (_d, mut v) = vault();
        add(
            &v,
            fields(
                "article",
                "OLD",
                &["author=Arad, Dana", "year=2025", "title=SAEs Are Good"],
            ),
        )
        .unwrap();
        set_tags(&mut v, "OLD", &["topics:sae".into()], &[]).unwrap();
        set_status(&mut v, "OLD", Status::Reading).unwrap();
        set_stars(&mut v, "OLD", Some(4)).unwrap();

        // preview reports the change but writes nothing
        let preview = rekey_preview(&v, None).unwrap();
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].citekey, "OLD");
        assert_eq!(preview[0].new_key, "arad2025saesAreGood");
        assert!(show(&v, "OLD").is_ok());

        // apply renames the entry and carries the sidecar over
        rekey_apply(&mut v, None).unwrap();
        assert!(show(&v, "OLD").is_err());
        let view = show(&v, "arad2025saesAreGood").unwrap();
        assert_eq!(view.tags, vec!["topics:sae".to_string()]);
        assert_eq!(view.status, "reading");
        assert_eq!(view.stars, Some(4));
    }

    #[test]
    fn rekey_disambiguates_colliding_entries() {
        let (_d, mut v) = vault();
        add(
            &v,
            fields(
                "article",
                "a",
                &["author=Gao, Leo", "year=2025", "title=Scaling Up"],
            ),
        )
        .unwrap();
        add(
            &v,
            fields(
                "article",
                "b",
                &["author=Gao, Leo", "year=2025", "title=Scaling Up"],
            ),
        )
        .unwrap();
        let changes = rekey_apply(&mut v, None).unwrap();
        let new_keys: Vec<&str> = changes.iter().map(|c| c.new_key.as_str()).collect();
        assert!(new_keys.contains(&"gao2025scalingUp"));
        assert!(new_keys.contains(&"gao2025scalingUpa"));
        assert!(changes.iter().any(|c| c.disambiguated));

        // Idempotent: a second pass is a clean no-op — the already-suffixed key
        // must not be reported as renaming to itself.
        assert!(rekey_preview(&v, None).unwrap().is_empty());
    }

    #[test]
    fn rekey_leaves_already_patterned_keys_untouched() {
        let (_d, v) = vault();
        add(
            &v,
            fields(
                "article",
                "arad2025saesAreGood", // already equals the pattern output
                &["author=Arad, Dana", "year=2025", "title=SAEs Are Good"],
            ),
        )
        .unwrap();
        assert!(rekey_preview(&v, None).unwrap().is_empty());
    }

    #[test]
    fn rekey_respects_a_pattern_override() {
        let (_d, v) = vault();
        add(
            &v,
            fields(
                "misc",
                "x",
                &["author=Bricken, Trenton", "year=2023", "title=Toward Mono"],
            ),
        )
        .unwrap();
        let preview = rekey_preview(&v, Some("{auth}{year}")).unwrap();
        assert_eq!(preview[0].new_key, "bricken2023");
    }

    // ----------------------------------------------------------- status / stars

    #[test]
    fn status_and_stars_set_show_and_prune() {
        let (_d, mut v) = vault();
        add(&v, fields("misc", "k", &[])).unwrap();
        set_status(&mut v, "k", Status::Done).unwrap();
        set_stars(&mut v, "k", Some(5)).unwrap();
        let view = show(&v, "k").unwrap();
        assert_eq!(view.status, "done");
        assert_eq!(view.stars, Some(5));

        // back to the defaults clears the sidecar entry entirely
        set_status(&mut v, "k", Status::Unread).unwrap();
        set_stars(&mut v, "k", Some(0)).unwrap();
        assert_eq!(show(&v, "k").unwrap().status, "unread");
        assert_eq!(show(&v, "k").unwrap().stars, None);
        assert!(!v.meta.contains_key("k"), "empty meta should be pruned");

        assert!(set_stars(&mut v, "k", Some(6))
            .unwrap_err()
            .contains("between 0 and 5"));
        assert!(set_status(&mut v, "ghost", Status::Done).is_err());
    }

    #[test]
    fn list_filters_by_status_and_stars() {
        let (_d, mut v) = vault();
        add(&v, fields("article", "a", &["title=A"])).unwrap();
        add(&v, fields("article", "b", &["title=B"])).unwrap();
        set_status(&mut v, "a", Status::Reading).unwrap();
        set_stars(&mut v, "a", Some(5)).unwrap();

        let reading = list(&v, Filter::Query("status:reading".into())).unwrap();
        assert_eq!(reading.len(), 1);
        assert_eq!(reading[0].citekey, "a");
        let unread = list(&v, Filter::Query("status:unread".into())).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].citekey, "b");
        let top = list(&v, Filter::Query("stars:>=4".into())).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].citekey, "a");
    }

    // ----------------------------------------------------------------- analyze

    #[test]
    fn analyze_flags_offline_health_issues() {
        let (_d, v) = vault();
        add(
            &v,
            AddSource::Bibtex(
                concat!(
                    "@article{a, title={A NICE PAPER}, journal={ICLR}}\n",
                    "@article{b, title={Another Paper}, journal={iclr}, year={2024}, url={http://x}}\n",
                    "@misc{c, title={A Good Title Here}, year={2023}, doi={10.1/x}}\n",
                    // d & e share a *consistently* spelled venue — must NOT be flagged
                    "@article{d, title={Paper Four}, journal={NeurIPS}, year={2023}, url={http://d}}\n",
                    "@article{e, title={Paper Five}, journal={NeurIPS}, year={2023}, url={http://e}}\n",
                )
                .into(),
            ),
        )
        .unwrap();
        let report = analyze(&v).unwrap();
        assert_eq!(report.total, 5);
        let by = |id: &str| &report.checks.iter().find(|c| c.id == id).unwrap().keys;
        assert!(by("titles").contains(&"a".to_string())); // ALL-CAPS shout
        assert!(by("year").contains(&"a".to_string()) && !by("year").contains(&"c".to_string()));
        assert!(by("url").contains(&"a".to_string()) && !by("url").contains(&"b".to_string()));
        // a & b share a venue spelled two ways (ICLR / iclr) → flagged …
        assert!(by("venues").contains(&"a".to_string()) && by("venues").contains(&"b".to_string()));
        // … but d & e share one consistent spelling (NeurIPS) → NOT flagged.
        assert!(
            !by("venues").contains(&"d".to_string()) && !by("venues").contains(&"e".to_string())
        );
    }

    // -------------------------------------------------- online enrich (pure parts)

    #[test]
    fn fill_missing_only_fills_absent_or_blank_fields() {
        let mut e = BibEntry::new("article", "k")
            .with_field("title", "Mine") // present — must be kept
            .with_field("doi", ""); // blank — should be filled
        let fetched = BibEntry::new("article", "x")
            .with_field("title", "Canonical") // must NOT overwrite
            .with_field("doi", "10.1/x")
            .with_field("year", "2020"); // absent — should be filled
        let filled = fill_missing(&mut e, &fetched);
        assert_eq!(e.get("title"), Some("Mine")); // not overwritten
        assert_eq!(e.get("doi"), Some("10.1/x")); // blank filled
        assert_eq!(e.get("year"), Some("2020")); // absent filled
        assert_eq!(filled, vec!["doi".to_string(), "year".to_string()]);
    }

    #[test]
    fn entry_doi_prefers_doi_field_then_doi_url() {
        let with_doi = BibEntry::new("misc", "k").with_field("doi", "10.1/x");
        assert_eq!(entry_doi(&with_doi).as_deref(), Some("10.1/x"));
        let with_url = BibEntry::new("misc", "k").with_field("url", "https://doi.org/10.2/y");
        assert_eq!(entry_doi(&with_url).as_deref(), Some("10.2/y"));
        let neither = BibEntry::new("misc", "k").with_field("url", "https://example.com/p");
        assert_eq!(entry_doi(&neither), None);
    }

    #[test]
    fn enrich_errors_before_any_network_call() {
        let (_d, v) = vault();
        add(&v, fields("misc", "k", &["title=T"])).unwrap(); // no doi / doi.org url
                                                             // No DOI → fails up front, never touching the network.
        assert!(enrich(&v, "k")
            .unwrap_err()
            .contains("no DOI to enrich from"));
        assert!(enrich(&v, "ghost")
            .unwrap_err()
            .contains("no entry with cite key 'ghost'"));
    }

    // --------------------------------------------------------------- dedupe

    #[test]
    fn dedupe_merges_fields_and_sidecar_keeping_the_richest() {
        let (_d, mut v) = vault();
        add(
            &v,
            AddSource::Bibtex(
                "@article{a, author={Vaswani, A}, year={2017}, title={Attention}, x={1}, extra={e}}"
                    .into(),
            ),
        )
        .unwrap();
        add(
            &v,
            AddSource::Bibtex(
                "@article{b, author={Vaswani, A}, year={2017}, title={Attention!}, y={2}}".into(),
            ),
        )
        .unwrap();
        set_tags(&mut v, "b", &["topics:nlp".into()], &[]).unwrap();
        set_stars(&mut v, "b", Some(5)).unwrap();

        // preview: one cluster, the richer `a` listed first (the keeper).
        let groups = dedupe_preview(&v).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].citekeys[0], "a");

        let merges = dedupe_merge(&mut v).unwrap();
        assert_eq!(
            merges,
            vec![DupMerge {
                kept: "a".into(),
                dropped: vec!["b".into()]
            }]
        );

        // `a` kept its own fields, gained `b`'s missing `y`, and inherited b's
        // tag + rating; `b` is gone.
        let a = show(&v, "a").unwrap();
        assert_eq!(a.fields.get("x").map(String::as_str), Some("1"));
        assert_eq!(a.fields.get("y").map(String::as_str), Some("2"));
        assert!(a.tags.contains(&"topics:nlp".to_string()));
        assert_eq!(a.stars, Some(5));
        assert!(show(&v, "b").is_err());
    }

    #[test]
    fn dedupe_fills_blank_keeper_fields_from_the_donor() {
        let (_d, mut v) = vault();
        // `a` looks richer by raw count but its doi/url are blank placeholders;
        // `b` is leaner but has the real values. The merge must keep them.
        add(
            &v,
            AddSource::Bibtex(
                "@article{a, author={Vaswani, A}, year={2017}, title={Attention}, doi={}, url={}}"
                    .into(),
            ),
        )
        .unwrap();
        add(
            &v,
            AddSource::Bibtex(
                "@article{b, author={Vaswani, A}, year={2017}, title={Attention}, doi={10.5555/real}, url={https://real}}"
                    .into(),
            ),
        )
        .unwrap();
        let merges = dedupe_merge(&mut v).unwrap();
        assert_eq!(merges.len(), 1);
        let kept = show(&v, &merges[0].kept).unwrap();
        assert_eq!(
            kept.fields.get("doi").map(String::as_str),
            Some("10.5555/real")
        );
        assert_eq!(
            kept.fields.get("url").map(String::as_str),
            Some("https://real")
        );
    }

    #[test]
    fn dedupe_is_a_noop_without_duplicates() {
        let (_d, mut v) = vault();
        add(
            &v,
            fields("article", "a", &["author=X, Y", "year=2020", "title=Alpha"]),
        )
        .unwrap();
        add(
            &v,
            fields("article", "b", &["author=Z, W", "year=2021", "title=Beta"]),
        )
        .unwrap();
        assert!(dedupe_preview(&v).unwrap().is_empty());
        assert!(dedupe_merge(&mut v).unwrap().is_empty());
        assert!(show(&v, "a").is_ok() && show(&v, "b").is_ok());
    }

    // ----------------------------------------------------- structured norm diffs

    #[test]
    fn normalize_preview_includes_field_diffs() {
        let (_d, v) = vault();
        add(
            &v,
            AddSource::Bibtex("@article{k, title={A  B}, abstract={x}}".into()),
        )
        .unwrap();
        let changes = normalize_preview(&v, None).unwrap();
        assert_eq!(changes.len(), 1);
        let diffs = &changes[0].diffs;
        // abstract is dropped (not on the keep-field whitelist)
        assert!(diffs
            .iter()
            .any(|d| d.field == "abstract" && d.to.is_none()));
        // title changed (whitespace tidy + capital protection)
        assert!(diffs
            .iter()
            .any(|d| d.field == "title" && d.from.as_deref() == Some("A  B")));
    }
}

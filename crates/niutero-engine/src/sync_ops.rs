//! Git sync and per-entry history: connect a vault to a remote, commit/pull/
//! push with a structured entry-level merge fallback, and trace one entry's
//! commits. Requires `git` on PATH; the offline core never calls in here.

use serde::Serialize;

use niutero_bib::{entries, entry_line_span, parse, BibItem};
use niutero_core::BibEntry;
use niutero_sync as git;
use niutero_vault::Vault;

use crate::{lock_vault, read_items, sync_prefs, write_items};

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
        return Err("not a git repository — run `niutero-cli connect <url>` first".into());
    }
    if git::remote_url(&v.root, "origin").is_none() {
        return Err("no 'origin' remote — run `niutero-cli connect <url>` first".into());
    }
    ensure_repo_hygiene(v)?;
    // Machine-local sync strategy (#48): pull/push are per-machine toggles.
    let prefs = sync_prefs(v)?;
    let message = message.unwrap_or_else(|| auto_commit_message(v));
    let committed = git::commit_all(&v.root, &message)?;
    let mut merged = false;
    if prefs.pull && git::has_upstream(&v.root) && git::pull(&v.root)? == git::PullOutcome::Conflict
    {
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
    if prefs.push {
        git::push(&v.root)?;
    }
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

/// The vault's `origin` remote URL, if it is a git repo with one configured —
/// what Settings → Sync shows instead of an empty field.
pub fn remote_url(v: &Vault) -> Option<String> {
    if !git::is_repo(&v.root) {
        return None;
    }
    git::remote_url(&v.root, "origin")
}

/// Commit the vault (no pull/push) when the library's `workflow.auto_commit`
/// is on and the vault is a git repo. Returns `Some(message)` when a commit
/// was made; `None` when the pref is off, the vault isn't a repo, or there
/// was nothing to commit — all without touching the network. Hooks call this
/// after a successful mutation; failures surface, they're never silent.
pub fn auto_commit_if_enabled(v: &Vault) -> Result<Option<String>, String> {
    if !v.config.workflow.auto_commit || !git::is_repo(&v.root) {
        return Ok(None);
    }
    ensure_repo_hygiene(v)?;
    let msg = auto_commit_message(v);
    if git::commit_all(&v.root, &msg)? {
        Ok(Some(msg))
    } else {
        Ok(None)
    }
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
pub(crate) struct EntryDiff {
    pub(crate) added: usize,
    pub(crate) changed: usize,
    pub(crate) removed: usize,
}

impl EntryDiff {
    pub(crate) fn message(&self) -> String {
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

pub(crate) fn entry_diff(old: &str, new: &str) -> EntryDiff {
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
        return Err("not a git repository — run `niutero-cli connect <url>` first".into());
    }
    let head = git::file_at_head(&v.root, "references.bib").ok_or_else(|| {
        "references.bib has no committed history yet — run `niutero-cli sync` first".to_string()
    })?;
    let (start, end) = match entry_line_span(&head, citekey) {
        Some(span) => span,
        // Not in the last commit: distinguish "added locally, not synced yet"
        // from "no such entry" by consulting the working tree.
        None => {
            let exists = entries(&read_items(v)?).any(|e| e.citekey == citekey);
            return Err(if exists {
                format!("'{citekey}' isn't in the last commit yet — run `niutero-cli sync` first")
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

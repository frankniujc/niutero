//! Tags / notes / status / stars — the sidecar-only metadata operations.
//! Everything here writes `.niutero/meta.json` and **never** touches
//! `references.bib` (invariant: the `.bib` stays niutero-agnostic).

use niutero_vault::{Status, Vault};

use crate::{entry_exists, lock_vault, prune_meta, save_sidecar};

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

/// Add tags to many entries in **one** sidecar write — the batched form of
/// [`set_tags`] for wizard-scale applies (the per-entry form rewrites and
/// fsyncs the whole sidecar once per entry, which freezes a UI on large
/// libraries). Same capability as repeated `tag --add`, just batched. Unknown
/// citekeys are collected, not fatal. Returns `(entries_changed, unknown_keys)`.
/// Sidecar only — `references.bib` is never touched.
pub fn set_tags_bulk(
    v: &mut Vault,
    adds: &[(String, Vec<String>)],
) -> Result<(usize, Vec<String>), String> {
    let _lock = lock_vault(v)?;
    let mut changed = 0usize;
    let mut unknown = Vec::new();
    for (citekey, add) in adds {
        if entry_exists(v, citekey).is_err() {
            unknown.push(citekey.clone());
            continue;
        }
        let meta = v.meta.entry(citekey.clone()).or_default();
        let before = meta.tags.clone();
        for t in add {
            if !t.is_empty() && !meta.tags.iter().any(|x| x == t) {
                meta.tags.push(t.clone());
            }
        }
        meta.tags.sort();
        meta.tags.dedup();
        if meta.tags != before {
            changed += 1;
        }
        prune_meta(v, citekey);
    }
    if changed > 0 {
        save_sidecar(v)?;
    }
    Ok((changed, unknown))
}

/// Every tag in use across the library with its entry count, sorted by name.
/// Derived from the sidecar (tags never live in `references.bib`).
pub fn list_tags(v: &Vault) -> Vec<(String, usize)> {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for m in v.meta.values() {
        for t in &m.tags {
            *counts.entry(t.clone()).or_insert(0) += 1;
        }
    }
    counts.into_iter().collect()
}

/// Rename a tag everywhere it occurs in the sidecar. This also serves as a
/// **merge**: if `to` already exists on an entry that has `from`, the two
/// collapse into one. `references.bib` is never touched. Returns the number of
/// entries changed. Errors if `to` is empty.
pub fn rename_tag(v: &mut Vault, from: &str, to: &str) -> Result<usize, String> {
    let to = to.trim();
    if to.is_empty() {
        return Err("the new tag name must not be empty".into());
    }
    if from == to {
        return Ok(0);
    }
    let _lock = lock_vault(v)?;
    let keys: Vec<String> = v
        .meta
        .iter()
        .filter(|(_, m)| m.tags.iter().any(|t| t == from))
        .map(|(k, _)| k.clone())
        .collect();
    for k in &keys {
        if let Some(m) = v.meta.get_mut(k) {
            m.tags.retain(|t| t != from);
            if !m.tags.iter().any(|t| t == to) {
                m.tags.push(to.to_string());
            }
            m.tags.sort();
            m.tags.dedup();
        }
    }
    save_sidecar(v)?;
    Ok(keys.len())
}

/// Remove a tag from every entry that carries it (sidecar only). Returns the
/// number of entries changed. Entries left with no sidecar data are pruned.
pub fn delete_tag(v: &mut Vault, name: &str) -> Result<usize, String> {
    let _lock = lock_vault(v)?;
    let keys: Vec<String> = v
        .meta
        .iter()
        .filter(|(_, m)| m.tags.iter().any(|t| t == name))
        .map(|(k, _)| k.clone())
        .collect();
    for k in &keys {
        if let Some(m) = v.meta.get_mut(k) {
            m.tags.retain(|t| t != name);
        }
        prune_meta(v, k);
    }
    save_sidecar(v)?;
    Ok(keys.len())
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

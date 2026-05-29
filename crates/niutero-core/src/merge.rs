//! Three-way merge of bibliographic entries — the structured resolver behind
//! `sync`'s git-conflict handling.
//!
//! Given a common ancestor (`base`) and two divergent versions (`ours`,
//! `theirs`), it merges entry-by-entry and, within an entry, field-by-field:
//! a change made on only one side is taken; a change made identically on both
//! is taken; a change made *differently* on both is a [`Conflict`]. The result
//! is pure data — no IO — so the engine can decide whether to finalize the git
//! merge (no conflicts) or abort it.

use std::collections::{HashMap, HashSet};

use crate::BibEntry;

/// Why a merge could not be resolved automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Ours and theirs changed the same field (or the entry type) to different
    /// values.
    BothModified,
    /// One side modified the entry while the other deleted it.
    ModifyDelete,
}

/// One unresolved disagreement between `ours` and `theirs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub citekey: String,
    /// The conflicting field, or `None` for an entry-level conflict (a type
    /// change, or a modify/delete).
    pub field: Option<String>,
    pub kind: ConflictKind,
}

/// The outcome of [`merge`]. `merged` is meaningful only when `conflicts` is
/// empty; otherwise the caller should abort and let the user resolve by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    /// Merged entries in a deterministic order: ours' order first, then the
    /// entries only `theirs` added (in their order). Deleted entries are gone.
    pub merged: Vec<BibEntry>,
    pub conflicts: Vec<Conflict>,
}

impl Merge {
    /// True when the merge resolved cleanly with no human intervention needed.
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// Three-way merge `base` → (`ours`, `theirs`). Entries are matched by cite key
/// (each side is assumed to have unique keys, the library invariant).
pub fn merge(base: &[BibEntry], ours: &[BibEntry], theirs: &[BibEntry]) -> Merge {
    let bmap = by_key(base);
    let omap = by_key(ours);
    let tmap = by_key(theirs);

    // Output order: every key ours has (in order), then keys only theirs added.
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for e in ours {
        if seen.insert(&e.citekey) {
            order.push(&e.citekey);
        }
    }
    for e in theirs {
        if !omap.contains_key(e.citekey.as_str()) && seen.insert(&e.citekey) {
            order.push(&e.citekey);
        }
    }

    let mut merged = Vec::new();
    let mut conflicts = Vec::new();
    for key in order {
        let outcome = resolve_entry(
            key,
            bmap.get(key).copied(),
            omap.get(key).copied(),
            tmap.get(key).copied(),
            &mut conflicts,
        );
        if let Some(entry) = outcome {
            merged.push(entry);
        }
    }
    Merge { merged, conflicts }
}

fn by_key(entries: &[BibEntry]) -> HashMap<&str, &BibEntry> {
    entries.iter().map(|e| (e.citekey.as_str(), e)).collect()
}

/// Resolve one cite key. Returns the merged entry (or `None` if it should be
/// deleted), pushing any conflicts. On conflict the returned entry is a
/// placeholder (the merge will be aborted, so it is never written).
fn resolve_entry(
    key: &str,
    base: Option<&BibEntry>,
    ours: Option<&BibEntry>,
    theirs: Option<&BibEntry>,
    conflicts: &mut Vec<Conflict>,
) -> Option<BibEntry> {
    match (ours, theirs) {
        // Present on both sides.
        (Some(o), Some(t)) => {
            if o == t {
                return Some(o.clone()); // no change, or the same change
            }
            match base {
                Some(b) if o == b => Some(t.clone()), // only theirs changed it
                Some(b) if t == b => Some(o.clone()), // only ours changed it
                _ => {
                    // Both changed it (or both added it differently): merge fields.
                    let (entry, field_conflicts) = merge_fields(base, o, t);
                    for (field, kind) in field_conflicts {
                        conflicts.push(Conflict {
                            citekey: key.to_string(),
                            field,
                            kind,
                        });
                    }
                    Some(entry)
                }
            }
        }
        // Only ours has it.
        (Some(o), None) => match base {
            None => Some(o.clone()),   // ours added it
            Some(b) if o == b => None, // theirs deleted; ours unchanged → delete
            Some(_) => {
                // theirs deleted; ours modified → conflict
                conflicts.push(Conflict {
                    citekey: key.to_string(),
                    field: None,
                    kind: ConflictKind::ModifyDelete,
                });
                Some(o.clone())
            }
        },
        // Only theirs has it.
        (None, Some(t)) => match base {
            None => Some(t.clone()),   // theirs added it
            Some(b) if t == b => None, // ours deleted; theirs unchanged → delete
            Some(_) => {
                // ours deleted; theirs modified → conflict
                conflicts.push(Conflict {
                    citekey: key.to_string(),
                    field: None,
                    kind: ConflictKind::ModifyDelete,
                });
                None
            }
        },
        (None, None) => None, // unreachable: the key came from ours or theirs
    }
}

/// Field-by-field three-way merge of one entry. Returns the merged entry plus
/// the per-field (or entry-type) conflicts.
fn merge_fields(
    base: Option<&BibEntry>,
    ours: &BibEntry,
    theirs: &BibEntry,
) -> (BibEntry, Vec<(Option<String>, ConflictKind)>) {
    let mut conflicts = Vec::new();

    // Entry type (a `None` field marks an entry-level conflict).
    let base_type = base.map(BibEntry::entry_type);
    let merged_type = if ours.entry_type() == theirs.entry_type() {
        ours.entry_type()
    } else if base_type == Some(ours.entry_type()) {
        theirs.entry_type()
    } else if base_type == Some(theirs.entry_type()) {
        ours.entry_type()
    } else {
        conflicts.push((None, ConflictKind::BothModified));
        ours.entry_type()
    };
    let mut merged = BibEntry::new(merged_type, &ours.citekey);

    // Field names: ours' order, then fields only theirs has.
    let mut names: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for k in ours.fields.keys() {
        if seen.insert(k) {
            names.push(k);
        }
    }
    for k in theirs.fields.keys() {
        if seen.insert(k) {
            names.push(k);
        }
    }

    for name in names {
        let bv = base.and_then(|e| e.get(name));
        let ov = ours.get(name);
        let tv = theirs.get(name);
        let chosen = if ov == tv {
            ov // same value (or both absent / both deleted)
        } else if ov == bv {
            tv // only theirs changed this field
        } else if tv == bv {
            ov // only ours changed this field
        } else {
            conflicts.push((Some(name.to_string()), ConflictKind::BothModified));
            ov // placeholder
        };
        if let Some(value) = chosen {
            merged.set(name, value);
        }
    }
    (merged, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: &str, fields: &[(&str, &str)]) -> BibEntry {
        let mut entry = BibEntry::new("article", key);
        for (k, v) in fields {
            entry.set(*k, *v);
        }
        entry
    }

    #[test]
    fn disjoint_field_changes_merge_clean() {
        let base = [e("k", &[("title", "T"), ("year", "2020")])];
        let ours = [e("k", &[("title", "T2"), ("year", "2020")])]; // changed title
        let theirs = [e("k", &[("title", "T"), ("year", "2021")])]; // changed year
        let m = merge(&base, &ours, &theirs);
        assert!(m.is_clean(), "conflicts: {:?}", m.conflicts);
        assert_eq!(m.merged[0].get("title"), Some("T2"));
        assert_eq!(m.merged[0].get("year"), Some("2021"));
    }

    #[test]
    fn same_field_changed_differently_conflicts() {
        let base = [e("k", &[("title", "T")])];
        let ours = [e("k", &[("title", "Ours")])];
        let theirs = [e("k", &[("title", "Theirs")])];
        let m = merge(&base, &ours, &theirs);
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].citekey, "k");
        assert_eq!(m.conflicts[0].field.as_deref(), Some("title"));
        assert_eq!(m.conflicts[0].kind, ConflictKind::BothModified);
    }

    #[test]
    fn identical_change_on_both_sides_is_clean() {
        let base = [e("k", &[("title", "T")])];
        let ours = [e("k", &[("title", "Same")])];
        let theirs = [e("k", &[("title", "Same")])];
        let m = merge(&base, &ours, &theirs);
        assert!(m.is_clean());
        assert_eq!(m.merged[0].get("title"), Some("Same"));
    }

    #[test]
    fn additions_from_both_sides_are_kept() {
        let base = [e("k", &[("title", "T")])];
        let ours = [e("k", &[("title", "T")]), e("mine", &[("title", "M")])];
        let theirs = [e("k", &[("title", "T")]), e("yours", &[("title", "Y")])];
        let m = merge(&base, &ours, &theirs);
        assert!(m.is_clean());
        let keys: Vec<&str> = m.merged.iter().map(|e| e.citekey.as_str()).collect();
        assert_eq!(keys, vec!["k", "mine", "yours"]); // ours' order, then theirs-only
    }

    #[test]
    fn delete_on_one_side_unchanged_on_other_deletes() {
        let base = [e("k", &[("title", "T")]), e("gone", &[("title", "G")])];
        let ours = [e("k", &[("title", "T")])]; // ours deleted `gone`
        let theirs = [e("k", &[("title", "T")]), e("gone", &[("title", "G")])];
        let m = merge(&base, &ours, &theirs);
        assert!(m.is_clean());
        assert!(m.merged.iter().all(|e| e.citekey != "gone"));
    }

    #[test]
    fn delete_versus_modify_conflicts() {
        let base = [e("k", &[("title", "T")])];
        let ours: [BibEntry; 0] = []; // ours deleted k
        let theirs = [e("k", &[("title", "Changed")])]; // theirs modified k
        let m = merge(&base, &ours, &theirs);
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].kind, ConflictKind::ModifyDelete);
        assert_eq!(m.conflicts[0].field, None);
    }

    #[test]
    fn entry_type_change_on_one_side_is_taken() {
        let base = [e("k", &[("title", "T")])]; // article
        let mut ours = e("k", &[("title", "T")]);
        ours.set_type("inproceedings");
        let theirs = [e("k", &[("title", "T")])];
        let m = merge(&base, &[ours], &theirs);
        assert!(m.is_clean());
        assert_eq!(m.merged[0].entry_type(), "inproceedings");
    }

    #[test]
    fn entry_type_changed_both_ways_conflicts() {
        let base = [e("k", &[])];
        let mut ours = e("k", &[]);
        ours.set_type("inproceedings");
        let mut theirs = e("k", &[]);
        theirs.set_type("misc");
        let m = merge(&base, &[ours], &[theirs]);
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].field, None);
        assert_eq!(m.conflicts[0].kind, ConflictKind::BothModified);
    }
}

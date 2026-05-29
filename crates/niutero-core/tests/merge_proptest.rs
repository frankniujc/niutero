//! Property test for the 3-way merge: disjoint per-entry changes (each entry
//! touched by at most one side) must always merge cleanly to the changed value,
//! never conflicting and never losing data.

use niutero_core::merge::merge;
use niutero_core::BibEntry;
use proptest::prelude::*;

fn entry(key: &str, val: &str) -> BibEntry {
    BibEntry::new("article", key).with_field("title", val)
}

proptest! {
    #[test]
    fn disjoint_per_entry_changes_merge_without_conflict(
        // Per entry: (base value, side, new value). side 1 = ours changes it,
        // 2 = theirs changes it, 0 = neither. No entry is changed by both sides.
        specs in prop::collection::vec((0u32..1000, 0u8..3, 0u32..1000), 1..12)
    ) {
        let mut base = Vec::new();
        let mut ours = Vec::new();
        let mut theirs = Vec::new();
        for (i, (bval, side, nval)) in specs.iter().enumerate() {
            let key = format!("k{i}");
            let bv = bval.to_string();
            base.push(entry(&key, &bv));
            match side {
                1 => {
                    ours.push(entry(&key, &nval.to_string()));
                    theirs.push(entry(&key, &bv));
                }
                2 => {
                    ours.push(entry(&key, &bv));
                    theirs.push(entry(&key, &nval.to_string()));
                }
                _ => {
                    ours.push(entry(&key, &bv));
                    theirs.push(entry(&key, &bv));
                }
            }
        }

        let m = merge(&base, &ours, &theirs);
        prop_assert!(m.is_clean(), "unexpected conflicts: {:?}", m.conflicts);
        prop_assert_eq!(m.merged.len(), specs.len());
        for (i, (bval, side, nval)) in specs.iter().enumerate() {
            let key = format!("k{i}");
            let got = m
                .merged
                .iter()
                .find(|e| e.citekey == key)
                .and_then(|e| e.get("title"))
                .unwrap()
                .to_string();
            let expected = if *side == 0 { bval.to_string() } else { nval.to_string() };
            prop_assert_eq!(got, expected, "wrong value for {}", key);
        }
    }
}

proptest! {
    // A model-based check of the per-field three-way merge: each entry has two
    // fields, and base/ours/theirs choose each value independently from a small
    // range (so identical values, one-sided edits, and same-field-both-edited
    // conflicts all occur). The merge must match an independent per-field model,
    // including the exact number of conflicts.
    #[test]
    fn field_level_three_way_matches_the_model(
        specs in prop::collection::vec(
            (0u32..4, 0u32..4, 0u32..4, 0u32..4, 0u32..4, 0u32..4),
            1..8,
        )
    ) {
        let mk = |key: &str, a: u32, b: u32| {
            BibEntry::new("article", key)
                .with_field("a", a.to_string())
                .with_field("b", b.to_string())
        };
        let mut base = Vec::new();
        let mut ours = Vec::new();
        let mut theirs = Vec::new();
        for (i, (ba, bb, oa, ob, ta, tb)) in specs.iter().enumerate() {
            let key = format!("k{i}");
            base.push(mk(&key, *ba, *bb));
            ours.push(mk(&key, *oa, *ob));
            theirs.push(mk(&key, *ta, *tb));
        }

        let m = merge(&base, &ours, &theirs);

        // Independent per-field three-way model: Ok(value) or Err = conflict.
        let resolve = |b: u32, o: u32, t: u32| -> Result<u32, ()> {
            if o == t {
                Ok(o)
            } else if o == b {
                Ok(t)
            } else if t == b {
                Ok(o)
            } else {
                Err(())
            }
        };

        let mut expected_conflicts = 0usize;
        for (i, (ba, bb, oa, ob, ta, tb)) in specs.iter().enumerate() {
            let key = format!("k{i}");
            let ra = resolve(*ba, *oa, *ta);
            let rb = resolve(*bb, *ob, *tb);
            expected_conflicts += ra.is_err() as usize + rb.is_err() as usize;
            if ra.is_ok() && rb.is_ok() {
                let e = m.merged.iter().find(|e| e.citekey == key).unwrap();
                prop_assert_eq!(e.get("a").unwrap(), ra.unwrap().to_string());
                prop_assert_eq!(e.get("b").unwrap(), rb.unwrap().to_string());
            }
        }
        prop_assert_eq!(m.conflicts.len(), expected_conflicts);
        prop_assert_eq!(m.is_clean(), expected_conflicts == 0);
    }
}

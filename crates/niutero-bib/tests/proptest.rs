//! Generative round-trip properties. A canonical entry must survive
//! `serialize → parse` exactly, and serialization must be idempotent over
//! arbitrary lists of entries.

use niutero_bib::{parse, to_bibtex, to_bibtex_entries, BibItem};
use niutero_core::BibEntry;
use proptest::prelude::*;

// Generators restricted to characters that are already in canonical form, so
// the only thing under test is structural round-tripping (type/key/field
// names, order, values) — not the one-time reformatting of exotic input,
// which the golden tests in roundtrip.rs cover.
fn arb_type() -> impl Strategy<Value = String> {
    "[a-z]{1,12}"
}
fn arb_key() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9_:.-]{0,20}"
}
fn arb_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,12}"
}
// No structural chars ({ } " @ # \ newline); commas/parens are fine inside a
// brace-delimited value.
fn arb_value() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ,.:;()/?!'+=_-]{0,40}"
}

fn arb_entry() -> impl Strategy<Value = BibEntry> {
    (
        arb_type(),
        arb_key(),
        prop::collection::vec((arb_name(), arb_value()), 0..8),
    )
        .prop_map(|(typ, key, fields)| {
            let mut e = BibEntry::new(typ, key);
            for (k, v) in fields {
                e.set(k, v); // dedups by name, last wins — keeps e canonical
            }
            e
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(800))]

    /// A canonical entry round-trips byte-for-byte through serialize → parse.
    #[test]
    fn entry_roundtrips_exactly(e in arb_entry()) {
        let s = to_bibtex_entries(std::slice::from_ref(&e));
        let items = parse(&s);
        prop_assert_eq!(items.len(), 1);
        match &items[0] {
            BibItem::Entry(parsed) => prop_assert_eq!(parsed, &e),
            other => prop_assert!(false, "expected an entry, got {:?}", other),
        }
    }

    /// Serializing a list of canonical entries is a fixed point of parse∘serialize.
    #[test]
    fn serialize_is_idempotent(es in prop::collection::vec(arb_entry(), 0..12)) {
        let s1 = to_bibtex_entries(&es);
        let s2 = to_bibtex(&parse(&s1));
        prop_assert_eq!(s1, s2);
    }
}

//! Determinism / round-trip tests — the heart of M1.
//!
//! The serializer is canonical and idempotent: the first pass may reformat,
//! but `serialize(parse(·))` is a fixed point. These tests pin that down on a
//! tricky hand-written fixture, a large generated corpus, and explicit golden
//! examples. Drop a real library at `tests/fixtures/large.bib` to exercise it
//! too.

use niutero_bib::{entries, parse, to_bibtex};
use std::path::Path;

/// Assert `serialize(parse(·))` is a fixed point and return the canonical form.
fn canonicalize_idempotent(raw: &str) -> String {
    let s1 = to_bibtex(&parse(raw));
    let s2 = to_bibtex(&parse(&s1));
    assert_eq!(s1, s2, "serialization is not idempotent");
    s1
}

#[test]
fn fixture_structure_and_idempotence() {
    let raw = include_str!("fixtures/sample.bib");
    let items = parse(raw);
    assert_eq!(entries(&items).count(), 5, "expected 5 entries");
    assert_eq!(
        items.len(),
        9,
        "expected 9 items (5 entries + 4 verbatim: free text, @string, @preamble, @comment)"
    );

    let canon = canonicalize_idempotent(raw);

    // Entries are unchanged structurally after canonicalization.
    let before: Vec<_> = entries(&items).cloned().collect();
    let after: Vec<_> = entries(&parse(&canon)).cloned().collect();
    assert_eq!(before, after, "entries changed across canonicalization");

    // Spot-check that canonicalization did its job on a known entry.
    let shannon = parse(&canon)
        .into_iter()
        .find_map(|it| match it {
            niutero_bib::BibItem::Entry(e) if e.citekey == "shannon1948" => Some(e),
            _ => None,
        })
        .expect("shannon1948 present");
    assert_eq!(shannon.entry_type(), "article"); // lowercased from @Article
    assert_eq!(
        shannon.get("title"),
        Some("A Mathematical Theory of Communication") // de-quoted
    );
    assert_eq!(shannon.get("year"), Some("1948")); // bare number kept
                                                   // @string keeps its verbatim text (including original case).
    assert!(canon.contains("@STRING{acl = {Association for Computational Linguistics}}"));
}

#[test]
fn unicode_survives_roundtrip() {
    let raw = include_str!("fixtures/sample.bib");
    let canon = canonicalize_idempotent(raw);
    assert!(canon.contains("Café Mathématique"));
    assert!(canon.contains("中文 Titles"));
    assert!(canon.contains("Müller, Jörg and 李, 雷"));
}

#[test]
fn multiline_value_is_preserved_verbatim() {
    let raw = include_str!("fixtures/sample.bib");
    let canon = canonicalize_idempotent(raw);
    // the multi-line "and"-joined author list keeps its internal newlines
    assert!(canon.contains("Niu, Jingcheng  and\n      Yuan, Xingdi"));
}

#[test]
fn canonical_form_examples() {
    // lowercases entry type + field names; "x" and bare numbers become {x}
    assert_eq!(
        to_bibtex(&parse(r#"@ARTICLE{k, Title = "Hi", Year = 2020}"#)),
        "@article{k,\n  title = {Hi},\n  year = {2020}\n}\n"
    );
    // nested braces kept; field order preserved; trailing comma dropped
    assert_eq!(
        to_bibtex(&parse("@misc{k, b = {x {y} z}, a = {1},}")),
        "@misc{k,\n  b = {x {y} z},\n  a = {1}\n}\n"
    );
    // @string preserved verbatim (whitespace and case included)
    assert_eq!(
        to_bibtex(&parse("@STRING{ x = {y} }")),
        "@STRING{ x = {y} }\n"
    );
}

/// Build a large, varied corpus: quoted/braced/bare values, nested braces,
/// unicode, commas inside values, an interleaved `@string`, a month macro.
fn big_bib(n: usize) -> String {
    let templates = [
        "@Article{key§, title = \"Paper §\", year = 1984, author = {Doe, J. and Smith, A.}}",
        "@inproceedings{conf§,\n  title = {Study of {Item} §},\n  booktitle = {Proc. of Something},\n  pages = {1--§},\n}",
        "@book{book§, title = {Café § Naïve}, publisher = \"Pub\", year = {2001}}",
        "@string{abbr§ = {Abbrev §}}",
        "@misc{m§, note = {n,o,t,e §}, month = jan}",
    ];
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&templates[i % 5].replace('§', &i.to_string()));
        s.push_str("\n\n");
    }
    s
}

#[test]
fn large_generated_corpus_is_idempotent() {
    let n = 2000;
    let raw = big_bib(n);
    let items = parse(&raw);
    assert_eq!(items.len(), n, "every template is one top-level item");
    // 4 of every 5 templates are entries; 1 is an @string (verbatim).
    assert_eq!(entries(&items).count(), n - n / 5);
    canonicalize_idempotent(&raw);
}

#[test]
fn optional_large_fixture_is_idempotent() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/large.bib");
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            let count = entries(&parse(&raw)).count();
            eprintln!("large.bib: {count} entries");
            canonicalize_idempotent(&raw);
        }
        Err(_) => {
            eprintln!("(skipped: drop a real library at tests/fixtures/large.bib to exercise it)")
        }
    }
}

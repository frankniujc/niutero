//! Integration tests for offline normalization, focused on the default AI/ML
//! venue-canonicalization ruleset. These parse real-world-shaped `.bib` text
//! (the messy variants a Zotero export or a Google Scholar copy produces) and
//! assert that every spelling of a venue collapses to one canonical string.
//!
//! The big real library at `$NIUTERO_BIB_FIXTURE` (e.g. `~/Desktop/all.bib`)
//! exercises idempotence over the whole corpus when the env var is set.

use niutero_bib::{entries, parse};
use niutero_core::BibEntry;
use niutero_norm::{normalize_entry, NormConfig};

fn first_entry(src: &str) -> BibEntry {
    entries(&parse(src))
        .next()
        .cloned()
        .unwrap_or_else(|| panic!("no entry parsed from: {src}"))
}

/// Normalize the first entry of `src` with the default config.
fn norm(src: &str) -> BibEntry {
    normalize_entry(&first_entry(src), &NormConfig::default()).0
}

fn booktitle_of(src: &str) -> String {
    norm(src).get("booktitle").unwrap_or_default().to_string()
}

fn inproc(booktitle: &str) -> String {
    booktitle_of(&format!(
        "@inproceedings{{k, title = {{T}}, booktitle = {{{booktitle}}}, year = {{2024}}}}"
    ))
}

/// `normalize(normalize(e)) == normalize(e)` and the second pass is a clean no-op.
fn assert_idempotent(entry: &BibEntry, cfg: &NormConfig) {
    let (once, _) = normalize_entry(entry, cfg);
    let (twice, notes) = normalize_entry(&once, cfg);
    assert_eq!(once, twice, "not idempotent for '{}'", entry.citekey);
    assert!(
        notes.is_empty(),
        "second pass changed '{}': {notes:?}",
        entry.citekey
    );
}

#[test]
fn venue_variants_collapse_to_one_canonical() {
    // Each group: the canonical form, then the messy variants seen in the wild.
    let groups: &[(&str, &[&str])] = &[
        (
            "International Conference on Learning Representations (ICLR)",
            &[
                "International Conference on Learning Representations",
                "The Thirteenth International Conference on Learning Representations",
                "The Eleventh International Conference on Learning Representations",
                "ICLR",
                "Proc. of ICLR 2024",
            ],
        ),
        (
            "Advances in Neural Information Processing Systems (NeurIPS)",
            &[
                "Advances in Neural Information Processing Systems",
                "Thirty-Seventh Conference on Neural Information Processing Systems",
                "The Thirty-eighth Annual Conference on Neural Information Processing Systems",
                "NeurIPS",
                "NIPS", // historical alias
            ],
        ),
        (
            "Annual Meeting of the Association for Computational Linguistics (ACL)",
            &[
                "Proceedings of the 57th Annual Meeting of the Association for Computational Linguistics",
                "Proceedings of the 58th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)",
            ],
        ),
        (
            "Conference on Empirical Methods in Natural Language Processing (EMNLP)",
            &[
                "Proceedings of the 2021 Conference on Empirical Methods in Natural Language Processing",
                "Proceedings of the 2020 Conference on Empirical Methods in Natural Language Processing (EMNLP)",
            ],
        ),
        (
            "Conference of the North American Chapter of the Association for Computational Linguistics (NAACL)",
            &[
                "Proceedings of the 2021 Conference of the North American Chapter of the Association for Computational Linguistics: Human Language Technologies",
            ],
        ),
        (
            "International Conference on Machine Learning (ICML)",
            &[
                "Forty-Second International Conference on Machine Learning",
                "Proceedings of the 37th International Conference on Machine Learning",
            ],
        ),
        (
            "International Conference on Computational Linguistics (COLING)",
            &["Proceedings of the 29th International Conference on Computational Linguistics"],
        ),
        (
            "Findings of the Association for Computational Linguistics: EMNLP",
            &["Findings of the Association for Computational Linguistics: EMNLP 2023"],
        ),
        (
            "Conference on Computer Vision and Pattern Recognition (CVPR)",
            &["2023 IEEE/CVF Conference on Computer Vision and Pattern Recognition (CVPR)"],
        ),
        (
            // Both the pre-2022 ("Track on Datasets and Benchmarks") and the
            // current ("Datasets and Benchmarks Track") word orders, and the
            // bare-track form — none must collapse to the plain NeurIPS venue.
            "Advances in Neural Information Processing Systems Track on Datasets and Benchmarks (NeurIPS D\\&B)",
            &[
                "Thirty-seventh Conference on Neural Information Processing Systems Datasets and Benchmarks Track",
                "Advances in Neural Information Processing Systems Track on Datasets and Benchmarks",
            ],
        ),
    ];

    for (canonical, variants) in groups {
        for v in *variants {
            assert_eq!(
                &inproc(v),
                canonical,
                "variant {v:?} should canonicalize to {canonical:?}"
            );
        }
    }
}

#[test]
fn cvpr_wins_over_iccv_substring() {
    // This input CONTAINS the ICCV pattern ("international conference on computer
    // vision") as a substring, so it genuinely exercises first-match-wins: CVPR
    // must be ordered before ICCV or this collapses to ICCV.
    assert_eq!(
        inproc("IEEE International Conference on Computer Vision and Pattern Recognition"),
        "Conference on Computer Vision and Pattern Recognition (CVPR)"
    );
}

#[test]
fn sibling_and_colocated_venues_are_not_mislabeled() {
    // Distinct venues that merely share a phrase with a flagship must NOT be
    // rewritten to that flagship's canonical: ICTIR is not SIGIR, a SIGKDD
    // workshop is not KDD, a NeurIPS-prefixed workshop is not the main track.
    let plain_neurips = "Advances in Neural Information Processing Systems (NeurIPS)";
    for v in [
        "ACM SIGIR International Conference on Theory of Information Retrieval",
        "ACM SIGKDD Workshop on Knowledge Discovery and Data Mining from Sensor Data",
        "Neural Information Processing Systems Workshop on Foo",
    ] {
        let out = inproc(v);
        assert!(
            !out.contains("(SIGIR)"),
            "{v:?} wrongly tagged SIGIR: {out:?}"
        );
        assert!(!out.contains("(KDD)"), "{v:?} wrongly tagged KDD: {out:?}");
        assert_ne!(out, plain_neurips, "{v:?} wrongly collapsed to NeurIPS");
    }
}

#[test]
fn journals_canonicalize_and_non_ai_journals_are_left_alone() {
    let journal = |j: &str| {
        norm(&format!(
            "@article{{k, title = {{T}}, journal = {{{j}}}, year = {{2024}}}}"
        ))
        .get("journal")
        .unwrap_or_default()
        .to_string()
    };
    assert_eq!(
        journal("Transactions of the Association for Computational Linguistics"),
        "Transactions of the Association for Computational Linguistics (TACL)"
    );
    assert_eq!(
        journal("Journal of Machine Learning Research"),
        "Journal of Machine Learning Research (JMLR)"
    );
    // AAAI proceedings indexed (Zotero-style) as a journal still canonicalize.
    assert_eq!(
        journal("Proceedings of the AAAI Conference on Artificial Intelligence"),
        "AAAI Conference on Artificial Intelligence (AAAI)"
    );
    // A psycholinguistics journal is not an AI/ML venue — untouched.
    assert_eq!(
        journal("Journal of Memory and Language"),
        "Journal of Memory and Language"
    );
    assert_eq!(journal("Cognition"), "Cognition");
}

#[test]
fn google_scholar_messy_inproceedings() {
    // A typical Google Scholar export: lowercased venue, an `organization`
    // noise field, a bare `month`, no acronym on the venue.
    let src = r#"@inproceedings{vaswani2017attention,
  title = {Attention is all you need},
  author = {Vaswani, Ashish and Shazeer, Noam and Parmar, Niki},
  booktitle = {Advances in neural information processing systems},
  volume = {30},
  pages = {5998--6008},
  year = {2017},
  organization = {Curran Associates},
  abstract = {The dominant sequence transduction models...}
}"#;
    let out = norm(src);
    // venue canonicalized despite the all-lowercase Scholar spelling
    assert_eq!(
        out.get("booktitle"),
        Some("Advances in Neural Information Processing Systems (NeurIPS)")
    );
    // noise fields dropped, kept fields retained
    assert_eq!(out.get("organization"), None);
    assert_eq!(out.get("abstract"), None);
    assert_eq!(out.get("pages"), Some("5998--6008"));
    // first capitalized word in the title is brace-protected
    assert_eq!(out.get("title"), Some("{{Attention}} is all you need"));
}

#[test]
fn google_scholar_messy_acl_with_doi() {
    // Scholar/ACL mix: an ACL Anthology DOI is authoritative for the venue, and
    // the messy booktitle + ACL boilerplate publisher are cleaned up.
    let src = r#"@inproceedings{devlin2019bert,
  title = {{BERT}: Pre-training of Deep Bidirectional Transformers},
  author = {Devlin, Jacob and Chang, Ming-Wei and Lee, Kenton and Toutanova, Kristina},
  booktitle = {Proceedings of the 2019 Conference of the NAACL},
  publisher = {Association for Computational Linguistics},
  doi = {10.18653/v1/N19-1423},
  year = {2019}
}"#;
    let out = norm(src);
    assert_eq!(
        out.get("booktitle"),
        Some("Conference of the North American Chapter of the Association for Computational Linguistics (NAACL)")
    );
    assert_eq!(out.get("publisher"), None); // ACL boilerplate dropped
    assert_eq!(out.get("doi"), None);
    // the anthology DOI became an aclanthology.org URL
    assert_eq!(out.get("url"), Some("https://aclanthology.org/N19-1423"));
}

#[test]
fn canonicalize_off_keeps_the_full_name_and_only_tags_the_acronym() {
    let cfg = NormConfig {
        canonicalize_venues: false,
        ..NormConfig::default()
    };
    let e = first_entry(
        "@inproceedings{k, title = {T}, booktitle = {The Thirteenth International Conference on Learning Representations}, year = {2025}}",
    );
    let bt = normalize_entry(&e, &cfg)
        .0
        .get("booktitle")
        .unwrap()
        .to_string();
    // ordinal preserved (not collapsed), acronym appended
    assert!(bt.contains("Thirteenth"), "got: {bt}");
    assert!(bt.ends_with("(ICLR)"), "got: {bt}");
}

#[test]
fn messy_corpus_is_idempotent() {
    let corpus = r#"
@inproceedings{a, title = {Scaling Laws}, booktitle = {Thirty-Seventh Conference on Neural Information Processing Systems}, year = {2023}}
@inproceedings{b, title = {Attention}, booktitle = {Advances in neural information processing systems}, year = {2017}}
@inproceedings{c, title = {A Study}, booktitle = {Proceedings of the 57th Annual Meeting of the Association for Computational Linguistics}, year = {2019}}
@inproceedings{d, title = {Vision}, booktitle = {2023 IEEE/CVF Conference on Computer Vision and Pattern Recognition (CVPR)}, year = {2023}}
@article{e, title = {Theory}, journal = {Transactions of the Association for Computational Linguistics}, year = {2022}}
@article{f, title = {Memory Words}, journal = {Journal of Memory and Language}, year = {2010}}
@inproceedings{g, title = {Findings}, booktitle = {Findings of the Association for Computational Linguistics: EMNLP 2023}, year = {2023}}
@misc{h, title = {Preprint}, eprint = {2310.01234}, archiveprefix = {arXiv}, year = {2023}}
"#;
    let cfg = NormConfig::default();
    for e in entries(&parse(corpus)) {
        assert_idempotent(e, &cfg);
    }
}

/// Optional whole-library robustness check. Set `NIUTERO_BIB_FIXTURE` to a real
/// `.bib` (e.g. `~/Desktop/all.bib`) to run it; skipped otherwise. Asserts every
/// entry normalizes idempotently and nothing panics over real data.
#[test]
fn optional_real_library_is_idempotent() {
    let Ok(path) = std::env::var("NIUTERO_BIB_FIXTURE") else {
        eprintln!("(skipped: set NIUTERO_BIB_FIXTURE=/path/to/library.bib to run)");
        return;
    };
    let raw = std::fs::read_to_string(&path).expect("read NIUTERO_BIB_FIXTURE");
    let cfg = NormConfig::default();
    let items = parse(&raw);
    let mut total = 0;
    let mut changed = 0;
    for e in entries(&items) {
        total += 1;
        let (once, notes) = normalize_entry(e, &cfg);
        if !notes.is_empty() {
            changed += 1;
        }
        let (twice, second_notes) = normalize_entry(&once, &cfg);
        assert_eq!(once, twice, "not idempotent for '{}'", e.citekey);
        assert!(
            second_notes.is_empty(),
            "second pass changed '{}': {second_notes:?}",
            e.citekey
        );
    }
    eprintln!("real library: {total} entries, {changed} would change on first normalize");
    assert!(total > 0, "fixture had no entries");
}

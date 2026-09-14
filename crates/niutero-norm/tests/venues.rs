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

/// `normalize(normalize(e)) == normalize(e)`, the second pass is a clean no-op,
/// and the output passes the serializer's gate (`validate()`) — a rule must
/// never emit an entry that would corrupt the surrounding `.bib`.
fn assert_idempotent(entry: &BibEntry, cfg: &NormConfig) {
    let (once, _) = normalize_entry(entry, cfg);
    once.validate().unwrap_or_else(|e| {
        panic!(
            "normalize produced an invalid entry for '{}': {e}",
            entry.citekey
        )
    });
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
fn volume_annotation_with_protected_braces_stays_balanced() {
    // The NAACL-2019 shape with an inner protected group: the volume-strip
    // regex must never leave unbalanced braces behind (each pass used to add
    // a brace layer — real corruption).
    let cfg = NormConfig::default();
    for bt in [
        "Proceedings of the 5th Workshop on Foo (Volume 1: {{Long}} Papers)",
        // the regex can consume a brace group's `{` without its `}` when a
        // stray char follows "Papers" — the balance check must catch it
        "Proceedings of the 5th Workshop on Foo (Volume 1: {Long Papers2})",
    ] {
        let e = first_entry(&format!(
            "@inproceedings{{k, title = {{T}}, booktitle = {{{bt}}}, year = {{2019}}}}"
        ));
        let (once, _) = normalize_entry(&e, &cfg);
        once.validate()
            .unwrap_or_else(|err| panic!("unbalanced output for {bt:?}: {err}"));
        assert_idempotent(&e, &cfg);
    }
}

#[test]
fn volume_annotation_strips_before_bare_acronym_expansion() {
    // Spec order: the annotation strips FIRST, so the bare acronym expands to
    // the canonical name instead of degrading to the literal string "ACL".
    assert_eq!(
        inproc("ACL (Volume 1: Long Papers)"),
        "Annual Meeting of the Association for Computational Linguistics (ACL)"
    );
}

#[test]
fn canonicalize_off_disables_anthology_and_bare_acronym_collapse() {
    let cfg = NormConfig {
        canonicalize_venues: false,
        ..NormConfig::default()
    };
    // (a) an anthology DOI no longer overrides the whole booktitle — it only
    // feeds the appended acronym.
    let e = first_entry(
        "@inproceedings{k, title = {T}, booktitle = {Proc. of Something}, doi = {10.18653/v1/2024.acl-long.1}, year = {2024}}",
    );
    let bt = normalize_entry(&e, &cfg)
        .0
        .get("booktitle")
        .unwrap()
        .to_string();
    assert!(bt.contains("Something"), "original name lost: {bt}");
    assert!(bt.ends_with("(ACL)"), "acronym not appended: {bt}");
    // (b) a bare acronym stays bare.
    let e2 = first_entry("@inproceedings{k2, title = {T}, booktitle = {ICLR}, year = {2024}}");
    let bt2 = normalize_entry(&e2, &cfg)
        .0
        .get("booktitle")
        .unwrap()
        .to_string();
    assert_eq!(bt2, "ICLR");
    assert_idempotent(&e, &cfg);
    assert_idempotent(&e2, &cfg);
}

#[test]
fn workshop_and_joint_venues_are_never_whole_string_replaced() {
    // Decision (b): satellite events and joint proceedings keep their own
    // name — at most an appended acronym, never a whole-string replacement.
    let cases: &[(&str, &str)] = &[
        (
            "Proceedings of the IJCAI Workshop on Knowledge Graphs",
            // "IJCAI" already present as a word: kept verbatim, no tag
            "Proceedings of the IJCAI Workshop on Knowledge Graphs",
        ),
        (
            "Workshop at the International Conference on Machine Learning",
            // parent tag appended, original preserved
            "Workshop at the International Conference on Machine Learning (ICML)",
        ),
        (
            "IEEE International Conference on Computer Vision Workshops",
            "IEEE International Conference on Computer Vision Workshops (ICCV)",
        ),
        (
            "International Conference on Computer Vision Theory and Applications (VISAPP)",
            // a *different* venue's acronym in parens: untouched, no ICCV tag
            "International Conference on Computer Vision Theory and Applications (VISAPP)",
        ),
        (
            "Companion Proceedings of the Web Conference 2019",
            "Companion Proceedings of the Web Conference 2019 (WWW)",
        ),
        (
            "Proceedings of the 2019 Conference on Empirical Methods in Natural Language \
             Processing and the 9th International Joint Conference on Natural Language \
             Processing (EMNLP-IJCNLP)",
            // a joint proceedings: untouched
            "Proceedings of the 2019 Conference on Empirical Methods in Natural Language \
             Processing and the 9th International Joint Conference on Natural Language \
             Processing (EMNLP-IJCNLP)",
        ),
    ];
    let cfg = NormConfig::default();
    for (input, expected) in cases {
        assert_eq!(&inproc(input), expected, "for input {input:?}");
        let e = first_entry(&format!(
            "@inproceedings{{k, title = {{T}}, booktitle = {{{input}}}, year = {{2024}}}}"
        ));
        assert_idempotent(&e, &cfg);
    }
    // The LREV *journal* must not become the LREC conference.
    let e = first_entry(
        "@article{k, title = {T}, journal = {Language Resources and Evaluation}, year = {2020}}",
    );
    let out = normalize_entry(&e, &NormConfig::default()).0;
    assert_eq!(
        out.get("journal"),
        Some("Language Resources and Evaluation")
    );
    // ...while the conference itself still canonicalizes.
    assert_eq!(
        inproc("Proceedings of the Fourteenth International Conference on Language Resources and Evaluation"),
        "International Conference on Language Resources and Evaluation (LREC)"
    );
}

#[test]
fn cikm_ampersand_variants_canonicalize() {
    // ACM spells CIKM ≥2021 with "&" (Crossref sometimes ships "&amp;"); the
    // rules speak "and". All spellings must land on the one canonical name.
    let canonical = "ACM International Conference on Information and Knowledge Management (CIKM)";
    for v in [
        "CIKM '22: Proceedings of the 31st ACM International Conference on Information & Knowledge Management",
        "Proceedings of the 31st ACM International Conference on Information &amp; Knowledge Management",
        "Proceedings of the 31st ACM International Conference on Information \\& Knowledge Management",
        "Proceedings of the 29th ACM International Conference on Information and Knowledge Management",
    ] {
        assert_eq!(&inproc(v), canonical, "variant {v:?}");
    }
}

#[test]
fn speech_and_ai_venue_variants_canonicalize() {
    let cases: &[(&str, &str)] = &[
        (
            "Proceedings of the 2023 IEEE International Conference on Acoustics, Speech and Signal Processing",
            "IEEE International Conference on Acoustics, Speech and Signal Processing (ICASSP)",
        ),
        (
            "ICASSP 2023 - 2023 IEEE International Conference on Acoustics, Speech, and Signal Processing",
            "IEEE International Conference on Acoustics, Speech and Signal Processing (ICASSP)",
        ),
        (
            "Interspeech 2023",
            "Annual Conference of the International Speech Communication Association (Interspeech)",
        ),
        (
            "Proceedings of the 26th European Conference on Artificial Intelligence",
            "European Conference on Artificial Intelligence (ECAI)",
        ),
        (
            "Proceedings of the 2nd Conference of the Asia-Pacific Chapter of the Association for Computational Linguistics",
            "Conference of the Asia-Pacific Chapter of the Association for Computational Linguistics (AACL)",
        ),
        (
            "Proceedings of the Eighth International Joint Conference on Natural Language Processing",
            "International Joint Conference on Natural Language Processing (IJCNLP)",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(&inproc(input), expected, "for input {input:?}");
    }
    // The AACL-IJCNLP joint spelling stays a joint (disjoint-venue guard).
    let joint = "Proceedings of the 2nd Conference of the Asia-Pacific Chapter of the \
                 Association for Computational Linguistics and the 12th International \
                 Joint Conference on Natural Language Processing";
    let out = inproc(joint);
    assert!(
        out.contains("Asia-Pacific") && out.contains("Joint Conference"),
        "joint proceedings collapsed: {out}"
    );
}

#[test]
fn arxiv_preprint_normalizes_to_canonical_misc() {
    let cfg = NormConfig::default();
    let expect_canonical = |src: &str, id: &str| {
        let e = first_entry(src);
        let (out, _) = normalize_entry(&e, &cfg);
        assert_eq!(out.entry_type(), "misc", "for {src}");
        assert_eq!(out.get("eprint"), Some(id), "for {src}");
        assert_eq!(out.get("archiveprefix"), Some("arXiv"), "for {src}");
        assert_eq!(
            out.get("url").unwrap_or_default(),
            format!("https://arxiv.org/abs/{id}"),
            "for {src}"
        );
        assert_eq!(out.get("journal"), None, "for {src}");
        assert!(
            out.get("volume").is_none_or(|v| !v.starts_with("abs/")),
            "for {src}"
        );
        assert_idempotent(&e, &cfg);
    };
    // A: @misc with eprint + archiveprefix (gains the url)
    expect_canonical(
        "@misc{a, title = {T}, eprint = {2310.01234}, archiveprefix = {arXiv}, year = {2023}}",
        "2310.01234",
    );
    // B: journal = {ArXiv} with the id in volume = {abs/...}
    expect_canonical(
        "@article{b, title = {T}, journal = {ArXiv}, volume = {abs/2505.01325}, year = {2025}}",
        "2505.01325",
    );
    // C: journal = {arXiv:2005.14165 [cs]}
    expect_canonical(
        "@article{c, title = {T}, journal = {arXiv:2005.14165 [cs]}, year = {2020}}",
        "2005.14165",
    );
    // journal = {ArXiv} with no extractable id: retyped @misc, journal dropped
    let e = first_entry("@article{d, title = {T}, journal = {ArXiv}, year = {2024}}");
    let (out, _) = normalize_entry(&e, &cfg);
    assert_eq!(out.entry_type(), "misc");
    assert_eq!(out.get("journal"), None);
    assert_idempotent(&e, &cfg);
    // a published-venue entry with an eprint is untouched
    let e = first_entry(
        "@inproceedings{p, title = {T}, booktitle = {International Conference on Learning Representations}, eprint = {2310.01234}, year = {2024}}",
    );
    let (out, _) = normalize_entry(&e, &cfg);
    assert_eq!(out.entry_type(), "inproceedings");
    // a JSTOR eprint is not an arXiv id
    let e = first_entry(
        "@article{j, title = {T}, journal = {Econometrica}, eprint = {1912345}, eprinttype = {jstor}, year = {1990}}",
    );
    let (out, _) = normalize_entry(&e, &cfg);
    assert_eq!(out.entry_type(), "article");
    assert_eq!(out.get("journal"), Some("Econometrica"));
    // toggle off: nothing happens
    let off = NormConfig {
        normalize_arxiv: false,
        ..NormConfig::default()
    };
    let e = first_entry(
        "@article{z, title = {T}, journal = {ArXiv}, volume = {abs/2505.01325}, year = {2025}}",
    );
    let (out, _) = normalize_entry(&e, &off);
    assert_eq!(out.entry_type(), "article");
    assert_eq!(out.get("journal"), Some("ArXiv"));
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
        once.validate().unwrap_or_else(|err| {
            panic!(
                "normalize produced an invalid entry for '{}': {err}",
                e.citekey
            )
        });
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

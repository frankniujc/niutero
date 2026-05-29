//! niutero-norm — offline, propose-only normalization.
//!
//! A Rust port of the offline rules in the user's own `bib_fixer` (no Python,
//! no network). Pure logic over a [`BibEntry`]; returns the normalized entry
//! plus human-readable change notes. Every rule is idempotent, so re-running a
//! normalized entry is a no-op. Online enrichment is a separate concern.
//!
//! niutero stores field *values* without their outer delimiters, so the rules
//! here operate on the inner text directly (no brace add/strip needed).

use std::path::Path;
use std::sync::OnceLock;

use niutero_core::BibEntry;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// `.niutero/norm.toml`. Missing keys fall back to the defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NormConfig {
    /// Whitelist: any field whose (lowercased) name isn't here is dropped.
    pub keep_fields: Vec<String>,
    /// Truncate author lists longer than this to "... and others" (0 = off).
    pub max_authors: usize,
    /// Wrap capitalized title words in `{{...}}` to protect them from LaTeX.
    pub protect_title_caps: bool,
    /// Append conference acronyms to booktitle/journal; expand bare acronyms.
    pub conference_acronyms: bool,
    /// Collapse a recognized AI/ML venue to its one canonical name, dropping the
    /// ordinal / year / "Proceedings of the …" noise (e.g. every "Thirteenth
    /// International Conference on Learning Representations" → the same string).
    /// Requires `conference_acronyms`.
    pub canonicalize_venues: bool,
    /// Convert a `doi` field into a `url` (and drop the `doi`).
    pub doi_to_url: bool,
    /// Collapse runs of whitespace and trim each field value.
    pub tidy_whitespace: bool,
}

impl Default for NormConfig {
    fn default() -> Self {
        Self {
            keep_fields: KEEP_FIELDS.iter().map(|s| s.to_string()).collect(),
            max_authors: 25,
            protect_title_caps: true,
            conference_acronyms: true,
            canonicalize_venues: true,
            doi_to_url: true,
            tidy_whitespace: true,
        }
    }
}

impl NormConfig {
    /// Load `<niutero_dir>/norm.toml`, falling back to defaults if absent or
    /// unparseable (tolerant — normalization shouldn't fail over a config typo).
    pub fn load(niutero_dir: &Path) -> Self {
        match std::fs::read_to_string(niutero_dir.join("norm.toml")) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Write a documented default `norm.toml`, only if one isn't there.
    pub fn write_default_if_absent(niutero_dir: &Path) -> std::io::Result<()> {
        let path = niutero_dir.join("norm.toml");
        if path.exists() {
            return Ok(());
        }
        std::fs::write(path, DEFAULT_NORM_TOML)
    }
}

/// Fields kept; everything else is dropped.
const KEEP_FIELDS: &[&str] = &[
    "title",
    "author",
    "year",
    "booktitle",
    "journal",
    "volume",
    "number",
    "pages",
    "publisher",
    "series",
    "eprint",
    "primaryclass",
    "archiveprefix",
    "url",
    "howpublished",
    "school",
    "institution",
    "isbn",
    "issn",
    "note",
    "chapter",
    "edition",
    "eprinttype",
];

/// `(regex_pattern, acronym)` matched (case-insensitive) against the plain text
/// of a booktitle/journal. First match wins, so order matters: list the more
/// specific patterns first. Every acronym here has a [`CANONICAL_VENUE`] entry,
/// and each canonical string contains its own pattern so re-matching is a
/// fixed point (idempotence).
const CONFERENCE_RULES: &[(&str, &str)] = &[
    // --- NLP: ACL Anthology family (Findings first — more specific) ---
    (
        r"findings of the association for computational linguistics.*?emnlp",
        "Findings of EMNLP",
    ),
    (
        r"findings of the association for computational linguistics.*?naacl",
        "Findings of NAACL",
    ),
    (
        r"findings of the association for computational linguistics.*?eacl",
        "Findings of EACL",
    ),
    (
        r"findings of the association for computational linguistics.*?acl",
        "Findings of ACL",
    ),
    (
        r"north american chapter of the association for computational linguistics",
        "NAACL",
    ),
    (
        r"nations of the americas chapter of the association for computational linguistics",
        "NAACL",
    ),
    (
        r"european chapter of the association for computational linguistics",
        "EACL",
    ),
    (
        r"annual meeting of the association for computational linguistics",
        "ACL",
    ),
    (
        r"conference on empirical methods in natural language processing",
        "EMNLP",
    ),
    (
        r"conference on computational natural language learning",
        "CoNLL",
    ),
    (
        r"international conference on computational linguistics",
        "COLING",
    ),
    (
        r"joint international conference on computational linguistics",
        "COLING",
    ),
    (r"international workshop on semantic evaluation", "SemEval"),
    (r"conference on machine translation", "WMT"),
    (r"sigdial meeting on discourse and dialogue", "SIGdial"),
    (r"\bblackboxnlp\b", "BlackboxNLP"),
    (r"conference on language modeling", "COLM"),
    (
        r"transactions of the association for computational linguistics",
        "TACL",
    ),
    (r"^computational linguistics( \(cl\))?$", "CL"),
    // --- ML / AI (general) ---
    // Match both word orders: "...Systems Track on Datasets and Benchmarks"
    // (pre-2022) and "...Systems Datasets and Benchmarks Track" (current).
    (
        r"neural information processing systems.*datasets and benchmarks",
        "NeurIPS D\\&B",
    ),
    (
        r"conference on neural information processing systems",
        "NeurIPS",
    ),
    (
        r"advances in neural information processing systems",
        "NeurIPS",
    ),
    // Bare "Neural Information Processing Systems" (+ optional year) only — the
    // end anchor keeps a co-located workshop from collapsing to the main track.
    (
        r"^neural information processing systems(\s+\d{4})?$",
        "NeurIPS",
    ),
    (r"international conference on machine learning", "ICML"),
    (
        r"international conference on learning representations",
        "ICLR",
    ),
    (
        r"international conference on artificial intelligence and statistics",
        "AISTATS",
    ),
    (
        r"conference on uncertainty in artificial intelligence",
        "UAI",
    ),
    (r"conference on learning theory", "COLT"),
    (r"conference on causal learning and reasoning", "CLeaR"),
    (r"conference on robot learning", "CoRL"),
    (r"aaai conference on artificial intelligence", "AAAI"),
    (
        r"international joint conference on artificial intelligence",
        "IJCAI",
    ),
    (r"\bijcai\b", "IJCAI"),
    (r"journal of machine learning research", "JMLR"),
    (r"j\.\s*mach\.\s*learn\.\s*res\.", "JMLR"),
    (r"transactions on machine learning research", "TMLR"),
    // --- Computer vision ---
    (
        r"conference on computer vision and pattern recognition",
        "CVPR",
    ),
    // bib_fixer uses `(?! and)` here; Rust's regex has no look-ahead, but CVPR
    // is matched first (above), so a CVPR title never reaches this rule.
    (r"international conference on computer vision", "ICCV"),
    (r"european conference on computer vision", "ECCV"),
    (
        r"winter conference on applications of computer vision",
        "WACV",
    ),
    (r"british machine vision conference", "BMVC"),
    (
        r"transactions on pattern analysis and machine intelligence",
        "TPAMI",
    ),
    // --- IR / web / data mining ---
    // Specific to the flagship events — `acm .*` would also swallow siblings
    // (ICTIR, SIGKDD workshops), which canonicalization would then corrupt.
    (
        r"research and development in information retrieval",
        "SIGIR",
    ),
    (
        r"sigkdd .*conference on knowledge discovery and data mining",
        "KDD",
    ),
    (
        r"conference on information and knowledge management",
        "CIKM",
    ),
    (r"conference on web search and data mining", "WSDM"),
    (r"conference on recommender systems", "RecSys"),
    (r"the web conference", "WWW"),
    (r"world wide web conference", "WWW"),
    // --- language resources / misc ---
    (r"language resources and evaluation", "LREC"),
    (r"international conference on semantic computing", "ICSC"),
];

/// Acronym (as emitted by [`CONFERENCE_RULES`]) → the exact canonical venue
/// string it collapses to. Each value contains the acronym's own pattern, so a
/// second pass re-derives the same string (idempotence). A bare acronym (e.g.
/// `ICLR`) also resolves here, by the single-token name in parentheses.
const CANONICAL_VENUE: &[(&str, &str)] = &[
    // --- NLP ---
    (
        "ACL",
        "Annual Meeting of the Association for Computational Linguistics (ACL)",
    ),
    (
        "NAACL",
        "Conference of the North American Chapter of the Association for Computational Linguistics (NAACL)",
    ),
    (
        "EMNLP",
        "Conference on Empirical Methods in Natural Language Processing (EMNLP)",
    ),
    (
        "EACL",
        "Conference of the European Chapter of the Association for Computational Linguistics (EACL)",
    ),
    (
        "COLING",
        "International Conference on Computational Linguistics (COLING)",
    ),
    (
        "CoNLL",
        "Conference on Computational Natural Language Learning (CoNLL)",
    ),
    (
        "Findings of ACL",
        "Findings of the Association for Computational Linguistics: ACL",
    ),
    (
        "Findings of EMNLP",
        "Findings of the Association for Computational Linguistics: EMNLP",
    ),
    (
        "Findings of NAACL",
        "Findings of the Association for Computational Linguistics: NAACL",
    ),
    (
        "Findings of EACL",
        "Findings of the Association for Computational Linguistics: EACL",
    ),
    (
        "SemEval",
        "International Workshop on Semantic Evaluation (SemEval)",
    ),
    ("WMT", "Conference on Machine Translation (WMT)"),
    (
        "SIGdial",
        "Annual SIGdial Meeting on Discourse and Dialogue (SIGdial)",
    ),
    (
        "BlackboxNLP",
        "BlackboxNLP Workshop on Analyzing and Interpreting Neural Networks for NLP",
    ),
    ("COLM", "Conference on Language Modeling (COLM)"),
    (
        "TACL",
        "Transactions of the Association for Computational Linguistics (TACL)",
    ),
    ("CL", "Computational Linguistics (CL)"),
    // --- ML / AI ---
    (
        "NeurIPS",
        "Advances in Neural Information Processing Systems (NeurIPS)",
    ),
    (
        "NeurIPS D\\&B",
        "Advances in Neural Information Processing Systems Track on Datasets and Benchmarks (NeurIPS D\\&B)",
    ),
    ("ICML", "International Conference on Machine Learning (ICML)"),
    (
        "ICLR",
        "International Conference on Learning Representations (ICLR)",
    ),
    (
        "AISTATS",
        "International Conference on Artificial Intelligence and Statistics (AISTATS)",
    ),
    (
        "UAI",
        "Conference on Uncertainty in Artificial Intelligence (UAI)",
    ),
    ("COLT", "Conference on Learning Theory (COLT)"),
    (
        "CLeaR",
        "Conference on Causal Learning and Reasoning (CLeaR)",
    ),
    ("CoRL", "Conference on Robot Learning (CoRL)"),
    ("AAAI", "AAAI Conference on Artificial Intelligence (AAAI)"),
    (
        "IJCAI",
        "International Joint Conference on Artificial Intelligence (IJCAI)",
    ),
    ("JMLR", "Journal of Machine Learning Research (JMLR)"),
    ("TMLR", "Transactions on Machine Learning Research (TMLR)"),
    // --- Computer vision ---
    (
        "CVPR",
        "Conference on Computer Vision and Pattern Recognition (CVPR)",
    ),
    ("ICCV", "International Conference on Computer Vision (ICCV)"),
    ("ECCV", "European Conference on Computer Vision (ECCV)"),
    (
        "WACV",
        "Winter Conference on Applications of Computer Vision (WACV)",
    ),
    ("BMVC", "British Machine Vision Conference (BMVC)"),
    (
        "TPAMI",
        "IEEE Transactions on Pattern Analysis and Machine Intelligence (TPAMI)",
    ),
    // --- IR / web / data mining ---
    (
        "SIGIR",
        "ACM SIGIR Conference on Research and Development in Information Retrieval (SIGIR)",
    ),
    (
        "KDD",
        "ACM SIGKDD Conference on Knowledge Discovery and Data Mining (KDD)",
    ),
    (
        "CIKM",
        "ACM International Conference on Information and Knowledge Management (CIKM)",
    ),
    (
        "WSDM",
        "ACM International Conference on Web Search and Data Mining (WSDM)",
    ),
    ("RecSys", "ACM Conference on Recommender Systems (RecSys)"),
    ("WWW", "The Web Conference (WWW)"),
    // --- language resources / misc ---
    (
        "LREC",
        "International Conference on Language Resources and Evaluation (LREC)",
    ),
    (
        "ICSC",
        "International Conference on Semantic Computing (ICSC)",
    ),
];

/// `(anthology_id_pattern, acronym)` — an ACL Anthology DOI/URL is authoritative
/// for the venue; the acronym resolves through [`CANONICAL_VENUE`] so an
/// anthology-tagged entry collapses to the same string as a name-matched one.
const ACL_ANTHOLOGY_VENUE_RULES: &[(&str, &str)] = &[
    (r"^\d{4}\.acl-", "ACL"),
    (r"^\d{4}\.naacl-", "NAACL"),
    (r"^\d{4}\.emnlp-", "EMNLP"),
    (r"^\d{4}\.eacl-", "EACL"),
    (r"^\d{4}\.findings-acl", "Findings of ACL"),
    (r"^\d{4}\.findings-emnlp", "Findings of EMNLP"),
    (r"^\d{4}\.findings-naacl", "Findings of NAACL"),
    (r"^\d{4}\.findings-eacl", "Findings of EACL"),
    (r"^\d{4}\.lrec", "LREC"),
    (r"^\d{4}\.coling-", "COLING"),
    (r"^\d{4}\.conll-", "CoNLL"),
    (r"^\d{4}\.semeval-", "SemEval"),
    (r"^P\d\d-", "ACL"),
    (r"^N\d\d-", "NAACL"),
    (r"^D\d\d-", "EMNLP"),
    (r"^E\d\d-", "EACL"),
    (r"^K\d\d-", "CoNLL"),
];

const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "and", "but", "or", "nor", "for", "yet", "so", "in", "on", "at", "to", "by",
    "of", "up", "as", "if", "from", "with", "into", "over", "upon", "than", "via",
];

const CANONICAL_TERMS: &[(&str, &str)] = &[("t-sne", "t-SNE")];

/// Apply the offline rules; returns the normalized entry + change notes.
pub fn normalize_entry(entry: &BibEntry, cfg: &NormConfig) -> (BibEntry, Vec<String>) {
    let mut out = BibEntry::new(entry.entry_type(), &entry.citekey);
    let mut notes = Vec::new();

    let anth_acro = infer_acl_anthology_venue(entry);
    let mut doi_url: Option<String> = None;
    let mut has_url = false;

    for (name, value) in &entry.fields {
        // doi -> url (then drop the doi field)
        if name == "doi" && cfg.doi_to_url {
            doi_url = Some(doi_to_url(value));
            notes.push("converted doi to url".to_string());
            continue;
        }
        if name == "url" {
            has_url = true;
        }
        // keep-list filter
        if !cfg.keep_fields.iter().any(|k| k == name) {
            notes.push(format!("dropped '{name}'"));
            continue;
        }
        // drop the ACL "publisher" boilerplate
        if name == "publisher" && value.trim() == "Association for Computational Linguistics" {
            notes.push("dropped ACL publisher".to_string());
            continue;
        }

        let new_value = match name.as_str() {
            "author" if cfg.max_authors > 0 => clip_authors(value, cfg.max_authors),
            "title" if cfg.protect_title_caps => protect_title(value),
            "booktitle" if cfg.conference_acronyms => {
                normalize_booktitle(value, anth_acro, cfg.canonicalize_venues)
            }
            "journal" if cfg.conference_acronyms => {
                normalize_journal(value, cfg.canonicalize_venues)
            }
            _ => value.clone(),
        };
        let new_value = if cfg.tidy_whitespace {
            tidy(&new_value)
        } else {
            new_value
        };
        if &new_value != value {
            notes.push(format!("rewrote '{name}'"));
        }
        out.set(name, new_value);
    }

    if let Some(url) = doi_url {
        if !has_url {
            out.set("url", url);
            notes.push("added url from doi".to_string());
        }
    }

    (out, notes)
}

// ----------------------------------------------------------------- field rules

fn doi_to_url(doi: &str) -> String {
    let inner = doi.trim();
    if let Some(id) = inner.strip_prefix("10.18653/v1/") {
        format!("https://aclanthology.org/{id}")
    } else if inner.starts_with("http") {
        inner.to_string()
    } else {
        format!("https://doi.org/{inner}")
    }
}

fn clip_authors(value: &str, max: usize) -> String {
    let authors: Vec<&str> = value.split(" and ").map(str::trim).collect();
    if authors.len() > max {
        let mut kept: Vec<&str> = authors[..max].to_vec();
        kept.push("others");
        kept.join(" and ")
    } else {
        value.to_string()
    }
}

fn protect_title(value: &str) -> String {
    let plain = strip_all_braces(value);
    let canon = fix_canonical_terms(&plain);
    protect_capitals(&canon)
}

fn normalize_booktitle(value: &str, anth_acro: Option<&str>, canonicalize: bool) -> String {
    // An ACL Anthology id is authoritative and always collapses to canonical.
    if let Some(canon) = anth_acro.and_then(canonical_for_acronym) {
        return canon.to_string();
    }
    let plain = strip_all_braces(value);
    if let Some(expanded) = expand_bare_acronym(&plain) {
        return expanded;
    }
    if canonicalize {
        if let Some(canon) = canonical_venue(&plain) {
            return canon;
        }
    }
    // Unrecognized (or canonicalization off): clean up, then tag the acronym.
    let mut inner = capitalise_content_words(&strip_volume_annotation(value));
    if let Some(acro) = get_acronym(&inner) {
        if !already_has_acronym(&inner, acro) {
            inner = format!("{inner} ({acro})");
        }
    }
    inner
}

fn normalize_journal(value: &str, canonicalize: bool) -> String {
    let plain = strip_all_braces(value);
    if let Some(expanded) = expand_bare_acronym(&plain) {
        return expanded;
    }
    if canonicalize {
        if let Some(canon) = canonical_venue(&plain) {
            return canon;
        }
    }
    // Leave an unrecognized journal name alone, but tag a known acronym.
    let mut inner = value.to_string();
    if let Some(acro) = get_acronym(&inner) {
        if !already_has_acronym(&inner, acro) {
            inner = format!("{inner} ({acro})");
        }
    }
    inner
}

/// The canonical venue string for a recognized booktitle/journal, or `None`.
fn canonical_venue(plain: &str) -> Option<String> {
    canonical_for_acronym(get_acronym(plain)?).map(str::to_string)
}

fn canonical_for_acronym(acronym: &str) -> Option<&'static str> {
    CANONICAL_VENUE
        .iter()
        .find(|(a, _)| *a == acronym)
        .map(|(_, canon)| *canon)
}

// ------------------------------------------------------------------- helpers

fn strip_all_braces(s: &str) -> String {
    s.replace("{{", "")
        .replace("}}", "")
        .replace(['{', '}'], "")
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fix_canonical_terms(text: &str) -> String {
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let res = RES.get_or_init(|| {
        CANONICAL_TERMS
            .iter()
            .map(|(pat, canon)| {
                (
                    RegexBuilder::new(&regex::escape(pat))
                        .case_insensitive(true)
                        .build()
                        .unwrap(),
                    *canon,
                )
            })
            .collect()
    });
    let mut out = text.to_string();
    for (re, canon) in res {
        out = re.replace_all(&out, *canon).into_owned();
    }
    out
}

/// Wrap any not-yet-protected word containing an uppercase letter in `{{...}}`.
fn protect_capitals(title: &str) -> String {
    let b = title.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n + 8);
    let mut i = 0;
    let mut plain_start = 0;
    while i < n {
        if b[i] == b'{' {
            protect_plain(&title[plain_start..i], &mut out);
            let end = brace_block_end(b, i);
            out.push_str(&title[i..end]);
            i = end;
            plain_start = i;
        } else {
            i += 1;
        }
    }
    protect_plain(&title[plain_start..], &mut out);
    out
}

fn protect_plain(text: &str, out: &mut String) {
    for piece in split_keep_ws(text) {
        if piece.trim().is_empty() {
            out.push_str(piece);
        } else if piece.bytes().any(|c| c.is_ascii_uppercase()) {
            out.push_str("{{");
            out.push_str(piece);
            out.push_str("}}");
        } else {
            out.push_str(piece);
        }
    }
}

/// Index just past a `{{...}}` or `{...}` block starting at `start`.
fn brace_block_end(b: &[u8], start: usize) -> usize {
    let n = b.len();
    if start + 1 < n && b[start + 1] == b'{' {
        // {{ ... }}
        let mut i = start + 2;
        while i + 1 < n && !(b[i] == b'}' && b[i + 1] == b'}') {
            i += 1;
        }
        (i + 2).min(n)
    } else {
        // { ... }
        let mut i = start + 1;
        while i < n && b[i] != b'}' {
            i += 1;
        }
        (i + 1).min(n)
    }
}

/// Split into alternating non-whitespace / whitespace runs, preserving both.
fn split_keep_ws(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        let ws = b[i].is_ascii_whitespace();
        let start = i;
        while i < n && b[i].is_ascii_whitespace() == ws {
            i += 1;
        }
        out.push(&text[start..i]);
    }
    out
}

/// Title-case content words; leave `{{...}}` blocks and digit-leading words
/// alone; lowercase function words except the first word.
fn capitalise_content_words(booktitle: &str) -> String {
    let b = booktitle.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n + 8);
    let mut i = 0;
    let mut plain_start = 0;
    let mut word_index = 0usize;
    while i < n {
        if b[i] == b'{' && i + 1 < n && b[i + 1] == b'{' {
            cap_plain(&booktitle[plain_start..i], &mut out, &mut word_index);
            let end = brace_block_end(b, i);
            // count words inside the protected block for first-word tracking
            word_index += booktitle[i..end]
                .trim_matches('{')
                .trim_matches('}')
                .split_whitespace()
                .count();
            out.push_str(&booktitle[i..end]);
            i = end;
            plain_start = i;
        } else {
            i += 1;
        }
    }
    cap_plain(&booktitle[plain_start..], &mut out, &mut word_index);
    out
}

fn cap_plain(text: &str, out: &mut String, word_index: &mut usize) {
    for piece in split_keep_ws(text) {
        if piece.trim().is_empty() {
            out.push_str(piece);
            continue;
        }
        // punctuation-only or digit-leading: leave alone
        let first = piece.chars().next().unwrap();
        if !piece.chars().any(|c| c.is_ascii_alphabetic()) || first.is_ascii_digit() {
            out.push_str(piece);
            *word_index += 1;
            continue;
        }
        // split leading/trailing non-alphabetic punctuation off the core
        let lead: String = piece
            .chars()
            .take_while(|c| !c.is_ascii_alphabetic())
            .collect();
        let trail: String = piece
            .chars()
            .rev()
            .take_while(|c| !c.is_ascii_alphabetic())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let core = &piece[lead.len()..piece.len() - trail.len()];
        let new_core =
            if *word_index == 0 || !FUNCTION_WORDS.contains(&core.to_lowercase().as_str()) {
                capitalize_first(core)
            } else {
                core.to_lowercase()
            };
        out.push_str(&lead);
        out.push_str(&new_core);
        out.push_str(&trail);
        *word_index += 1;
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn get_acronym(field_value: &str) -> Option<&'static str> {
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let res = RES.get_or_init(|| {
        CONFERENCE_RULES
            .iter()
            .map(|(pat, acro)| {
                (
                    RegexBuilder::new(pat)
                        .case_insensitive(true)
                        .build()
                        .unwrap(),
                    *acro,
                )
            })
            .collect()
    });
    let plain = strip_all_braces(field_value);
    let plain = plain.trim();
    res.iter()
        .find(|(re, _)| re.is_match(plain))
        .map(|(_, a)| *a)
}

fn already_has_acronym(field_value: &str, acronym: &str) -> bool {
    let plain = strip_all_braces(field_value);
    let re = Regex::new(&format!(r"\([^)]*{}[^)]*\)", regex::escape(acronym))).unwrap();
    re.is_match(&plain)
}

fn expand_bare_acronym(plain: &str) -> Option<String> {
    // A bare acronym, optionally behind a "Proc. of" / "Proceedings of" / "In"
    // prefix and/or a trailing year — but ONLY when what remains is a single
    // token, so a full venue name can never be mistaken for an acronym here.
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        RegexBuilder::new(
            r"^(?:proc\.?\s+of\s+|proceedings\s+of\s+(?:the\s+)?|in\s+)?([A-Za-z]+)(?:\s+\d{4})?$",
        )
        .case_insensitive(true)
        .build()
        .unwrap()
    });
    let caps = re.captures(plain.trim())?;
    let mut token = caps.get(1)?.as_str().to_lowercase();
    if token == "nips" {
        token = "neurips".to_string(); // historical alias for NeurIPS
    }
    // The single-token acronym name (in CANONICAL_VENUE) resolves the bare form,
    // so a bare `ICLR` lands on the same canonical string as the full name.
    CANONICAL_VENUE
        .iter()
        .find(|(acro, _)| acro.to_lowercase() == token)
        .map(|(_, canon)| canon.to_string())
}

fn strip_volume_annotation(value: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        RegexBuilder::new(
            r"\s*[,(]\s*\{*Volume\}*\s*\d+\s*[:(]\s*\{*\w+\}*\s*(\{*and\}*\s*\{*\w+\}*\s*)?\{*Papers?\}*\s*\)*",
        )
        .case_insensitive(true)
        .build()
        .unwrap()
    });
    re.replace_all(value, "").into_owned()
}

fn infer_acl_anthology_venue(entry: &BibEntry) -> Option<&'static str> {
    let mut anth_id: Option<String> = None;
    if let Some(doi) = entry.get("doi") {
        if let Some(id) = doi.trim().strip_prefix("10.18653/v1/") {
            anth_id = Some(id.trim().trim_end_matches('/').to_string());
        }
    }
    if anth_id.is_none() {
        if let Some(url) = entry.get("url") {
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = RE.get_or_init(|| Regex::new(r"aclanthology\.org/([\w.\-/]+)").unwrap());
            if let Some(c) = re.captures(url) {
                let id = c.get(1).unwrap().as_str().trim_end_matches('/');
                let id = id
                    .strip_suffix(".pdf")
                    .or_else(|| id.strip_suffix(".bib"))
                    .unwrap_or(id);
                anth_id = Some(id.to_string());
            }
        }
    }
    let id = anth_id?;
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let res = RES.get_or_init(|| {
        ACL_ANTHOLOGY_VENUE_RULES
            .iter()
            .map(|(pat, acro)| (Regex::new(pat).unwrap(), *acro))
            .collect()
    });
    res.iter()
        .find(|(re, _)| re.is_match(&id))
        .map(|(_, acro)| *acro)
}

/// Documented default written by [`NormConfig::write_default_if_absent`].
const DEFAULT_NORM_TOML: &str = "\
# niutero offline normalization config (a port of bib_fixer's offline rules).
# `niutero normalize` is propose-only: it shows what would change; nothing is
# written without --write.

# Whitelist of fields to keep; any other field is dropped.
keep_fields = [
  \"title\", \"author\", \"year\", \"booktitle\", \"journal\", \"volume\", \"number\",
  \"pages\", \"publisher\", \"series\", \"eprint\", \"primaryclass\", \"archiveprefix\",
  \"url\", \"howpublished\", \"school\", \"institution\", \"isbn\", \"issn\", \"note\",
  \"chapter\", \"edition\", \"eprinttype\",
]

# Truncate author lists longer than this to '... and others' (0 = off).
max_authors = 25

# Wrap capitalized title words in {{...}} to protect them from LaTeX lowercasing.
protect_title_caps = true

# Append conference acronyms to booktitle/journal and expand bare acronyms.
conference_acronyms = true

# Collapse a recognized AI/ML venue to one canonical name, dropping ordinal /
# year / 'Proceedings of the ...' noise (needs conference_acronyms). Turn off to
# only append the acronym to the existing (cleaned) venue string.
canonicalize_venues = true

# Convert a `doi` field into a `url` (and drop the `doi`).
doi_to_url = true

# Collapse runs of whitespace and trim each field value.
tidy_whitespace = true
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_non_kept_fields_and_converts_doi() {
        let e = BibEntry::new("article", "k")
            .with_field("title", "Hello")
            .with_field("abstract", "long text")
            .with_field("doi", "10.1234/xyz");
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        assert_eq!(out.get("abstract"), None);
        assert_eq!(out.get("doi"), None);
        assert_eq!(out.get("url"), Some("https://doi.org/10.1234/xyz"));
    }

    #[test]
    fn protects_title_capitals() {
        let e = BibEntry::new("article", "k").with_field("title", "Hello World of GPT");
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        assert_eq!(out.get("title"), Some("{{Hello}} {{World}} of {{GPT}}"));
    }

    #[test]
    fn clips_long_author_lists() {
        let authors = (0..30)
            .map(|i| format!("A{i}, X"))
            .collect::<Vec<_>>()
            .join(" and ");
        let e = BibEntry::new("article", "k").with_field("author", authors);
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        let got = out.get("author").unwrap();
        assert!(got.ends_with(" and others"));
        assert_eq!(got.split(" and ").count(), 26); // 25 + "others"
    }

    #[test]
    fn appends_conference_acronym_and_expands_bare() {
        let e = BibEntry::new("inproceedings", "k").with_field("booktitle", "ICLR");
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        let bt = out.get("booktitle").unwrap();
        assert!(bt.contains("Learning Representations"));
        assert!(bt.contains("(ICLR)"));
    }

    #[test]
    fn acl_anthology_doi_sets_venue() {
        let e = BibEntry::new("inproceedings", "k")
            .with_field("booktitle", "Proc. of something")
            .with_field("doi", "10.18653/v1/2024.acl-long.1");
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        let bt = out.get("booktitle").unwrap();
        assert!(bt.contains("(ACL)"), "got: {bt}");
        // doi became an aclanthology url
        assert_eq!(
            out.get("url"),
            Some("https://aclanthology.org/2024.acl-long.1")
        );
    }

    #[test]
    fn is_idempotent() {
        let e = BibEntry::new("inproceedings", "k")
            .with_field("title", "Llama See, Llama Do")
            .with_field(
                "booktitle",
                "Annual Meeting of the Association for Computational Linguistics",
            )
            .with_field("author", "Niu, Jingcheng and Yuan, Xingdi")
            .with_field("abstract", "x");
        let cfg = NormConfig::default();
        let (once, _) = normalize_entry(&e, &cfg);
        let (twice, notes) = normalize_entry(&once, &cfg);
        assert_eq!(once, twice);
        assert!(notes.is_empty(), "second pass changed something: {notes:?}");
    }

    #[test]
    fn every_canonical_venue_is_a_fixed_point() {
        // Each canonical string must be re-recognized as its own acronym, so a
        // second normalize is a no-op. This guards idempotence for *every* venue
        // (not just the ones a sample library happens to contain).
        let cfg = NormConfig::default();
        for (acro, canonical) in CANONICAL_VENUE {
            let e = BibEntry::new("inproceedings", "k")
                .with_field("title", "T")
                .with_field("booktitle", *canonical);
            let (out, _) = normalize_entry(&e, &cfg);
            assert_eq!(
                out.get("booktitle"),
                Some(*canonical),
                "canonical venue for '{acro}' is not a fixed point",
            );
        }
    }

    #[test]
    fn canonical_lookup_exists_for_every_conference_rule() {
        // Every acronym a CONFERENCE_RULE can emit must resolve to a canonical
        // string, or canonicalization would silently fall back to append-only.
        for (pat, acro) in CONFERENCE_RULES {
            assert!(
                canonical_for_acronym(acro).is_some(),
                "no CANONICAL_VENUE for acronym '{acro}' (rule {pat:?})",
            );
        }
    }

    #[test]
    fn default_toml_matches_default_config() {
        let parsed: NormConfig = toml::from_str(DEFAULT_NORM_TOML).unwrap();
        let d = NormConfig::default();
        assert_eq!(parsed.keep_fields, d.keep_fields);
        assert_eq!(parsed.max_authors, d.max_authors);
        assert_eq!(parsed.conference_acronyms, d.conference_acronyms);
        assert_eq!(parsed.canonicalize_venues, d.canonicalize_venues);
        assert_eq!(parsed.doi_to_url, d.doi_to_url);
        assert_eq!(parsed.protect_title_caps, d.protect_title_caps);
        assert_eq!(parsed.tidy_whitespace, d.tidy_whitespace);
    }
}

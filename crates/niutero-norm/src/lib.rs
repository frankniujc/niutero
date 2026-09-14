//! niutero-norm — offline, propose-only normalization.
//!
//! A Rust port of the offline rules in the user's own `bib_fixer` (no Python,
//! no network). Pure logic over a [`BibEntry`]; returns the normalized entry
//! plus human-readable change notes. Every rule is idempotent, so re-running a
//! normalized entry is a no-op. Online enrichment is a separate concern.
//!
//! niutero stores field *values* without their outer delimiters, so the rules
//! here operate on the inner text directly (no brace add/strip needed).

use std::collections::HashMap;
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
    /// Decode stray HTML entities (`&amp;` — Crossref/ACM BibTeX carries them)
    /// and escape a bare `&` to `\&` so values are LaTeX-safe.
    pub fix_entities: bool,
    /// Collapse arXiv preprints to one canonical shape: `@misc` with
    /// `eprint`/`archiveprefix`/`url`, dropping `journal = {ArXiv}`-style noise.
    /// Entries with a real venue are never touched.
    pub normalize_arxiv: bool,
    /// Named alternative profiles (`[profiles.<name>]` in `norm.toml`), selected
    /// with `normalize --profile <name>`. Each is a *full* config: any key it
    /// omits falls back to the built-in default (not to the base above).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub profiles: HashMap<String, NormConfig>,
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
            fix_entities: true,
            normalize_arxiv: true,
            profiles: HashMap::new(),
        }
    }
}

impl NormConfig {
    /// Load `<niutero_dir>/norm.toml`. Absent → defaults; unreadable or
    /// unparseable → an error. A config typo must fail loudly: silently
    /// reverting every rule to its default and then rewriting the library is
    /// far worse than refusing to run.
    pub fn load(niutero_dir: &Path) -> Result<Self, String> {
        let path = niutero_dir.join("norm.toml");
        let mut cfg = match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str::<Self>(&s).map_err(|e| format!("norm.toml: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(format!("norm.toml: {e}")),
        };
        cfg.normalize_self()?;
        Ok(cfg)
    }

    /// Lowercase `keep_fields` (field names always compare lowercased) and
    /// reject an empty whitelist — it would strip every field from every entry,
    /// which is never what a config edit meant. Recurses into profiles.
    fn normalize_self(&mut self) -> Result<(), String> {
        if self.keep_fields.is_empty() {
            return Err(
                "norm.toml: keep_fields is empty — that would drop every field from every \
                 entry; delete the key to use the defaults"
                    .into(),
            );
        }
        for f in &mut self.keep_fields {
            *f = f.to_lowercase();
        }
        for (name, p) in &mut self.profiles {
            p.normalize_self()
                .map_err(|e| format!("{e} (profile '{name}')"))?;
        }
        Ok(())
    }

    /// The config to normalize with: the base config (`profile = None`) or a
    /// named `[profiles.<name>]`. Errors if a requested profile isn't defined.
    pub fn resolve(niutero_dir: &Path, profile: Option<&str>) -> Result<Self, String> {
        let base = Self::load(niutero_dir)?;
        match profile {
            None => Ok(base),
            Some(name) => base
                .profiles
                .get(name)
                .cloned()
                .ok_or_else(|| format!("no normalize profile '{name}' in norm.toml")),
        }
    }

    /// Write a documented default `norm.toml`, only if one isn't there.
    pub fn write_default_if_absent(niutero_dir: &Path) -> std::io::Result<()> {
        let path = niutero_dir.join("norm.toml");
        if path.exists() {
            return Ok(());
        }
        std::fs::write(path, Self::default().to_toml())
    }

    /// Persist this config as `<niutero_dir>/norm.toml` (documented form).
    /// Validates first (an empty `keep_fields` is refused, names lowercased).
    pub fn save(&self, niutero_dir: &Path) -> Result<(), String> {
        let mut cfg = self.clone();
        cfg.normalize_self()?;
        std::fs::write(niutero_dir.join("norm.toml"), cfg.to_toml())
            .map_err(|e| format!("write norm.toml: {e}"))
    }

    /// The option names [`set_option`](Self::set_option) accepts, in display
    /// order (the GUI Ruleset and `norm-config` share this list).
    pub const OPTION_KEYS: &'static [&'static str] = &[
        "keep_fields",
        "max_authors",
        "protect_title_caps",
        "conference_acronyms",
        "canonicalize_venues",
        "doi_to_url",
        "tidy_whitespace",
        "fix_entities",
        "normalize_arxiv",
    ];

    /// Set one option from its textual form (`norm-config --set key=value`,
    /// the GUI toggles): booleans as `true`/`false`, `max_authors` as an
    /// integer (`0` = off), `keep_fields` as a comma-separated list.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        let v = value.trim();
        let flag = |v: &str| -> Result<bool, String> {
            match v.to_ascii_lowercase().as_str() {
                "true" | "on" | "yes" | "1" => Ok(true),
                "false" | "off" | "no" | "0" => Ok(false),
                other => Err(format!("'{other}' is not a boolean (true/false)")),
            }
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "keep_fields" => {
                let list: Vec<String> = v
                    .split(',')
                    .map(|f| f.trim().to_lowercase())
                    .filter(|f| !f.is_empty())
                    .collect();
                if list.is_empty() {
                    return Err("keep_fields must list at least one field".into());
                }
                self.keep_fields = list;
            }
            "max_authors" => {
                self.max_authors = v
                    .parse()
                    .map_err(|_| format!("'{v}' is not a number for max_authors"))?;
            }
            "protect_title_caps" => self.protect_title_caps = flag(v)?,
            "conference_acronyms" => self.conference_acronyms = flag(v)?,
            "canonicalize_venues" => self.canonicalize_venues = flag(v)?,
            "doi_to_url" => self.doi_to_url = flag(v)?,
            "tidy_whitespace" => self.tidy_whitespace = flag(v)?,
            "fix_entities" => self.fix_entities = flag(v)?,
            "normalize_arxiv" => self.normalize_arxiv = flag(v)?,
            other => {
                return Err(format!(
                    "unknown normalize option '{other}' (want one of: {})",
                    Self::OPTION_KEYS.join(", ")
                ))
            }
        }
        Ok(())
    }

    /// Render as a documented `norm.toml`: the scaffolded file's comments with
    /// this config's values, profiles appended as plain tables. Parses back to
    /// an equal config.
    pub fn to_toml(&self) -> String {
        let keep = self
            .keep_fields
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = String::new();
        out.push_str(
            "# niutero offline normalization config (a port of bib_fixer's offline rules).\n\
             # `niutero normalize` is propose-only: it shows what would change; nothing is\n\
             # written without --write. Edit by hand or with `niutero-cli norm-config`.\n\n",
        );
        out.push_str("# Whitelist of fields to keep; any other field is dropped.\n");
        out.push_str(&format!("keep_fields = [{keep}]\n\n"));
        out.push_str("# Truncate author lists longer than this to '... and others' (0 = off).\n");
        out.push_str(&format!("max_authors = {}\n\n", self.max_authors));
        out.push_str(
            "# Wrap capitalized title words in {{...}} to protect them from LaTeX lowercasing.\n",
        );
        out.push_str(&format!(
            "protect_title_caps = {}\n\n",
            self.protect_title_caps
        ));
        out.push_str(
            "# Append conference acronyms to booktitle/journal and expand bare acronyms.\n",
        );
        out.push_str(&format!(
            "conference_acronyms = {}\n\n",
            self.conference_acronyms
        ));
        out.push_str(
            "# Collapse a recognized AI/ML venue to one canonical name, dropping ordinal /\n\
             # year / 'Proceedings of the ...' noise (needs conference_acronyms). Turn off to\n\
             # only append the acronym to the existing (cleaned) venue string.\n",
        );
        out.push_str(&format!(
            "canonicalize_venues = {}\n\n",
            self.canonicalize_venues
        ));
        out.push_str("# Convert a `doi` field into a `url` (and drop the `doi`).\n");
        out.push_str(&format!("doi_to_url = {}\n\n", self.doi_to_url));
        out.push_str("# Collapse runs of whitespace and trim each field value.\n");
        out.push_str(&format!("tidy_whitespace = {}\n\n", self.tidy_whitespace));
        out.push_str(
            "# Decode stray HTML entities (&amp; ...) and escape a bare `&` to `\\&` so\n\
             # values are LaTeX-safe (Crossref/ACM BibTeX needs this).\n",
        );
        out.push_str(&format!("fix_entities = {}\n\n", self.fix_entities));
        out.push_str(
            "# Collapse arXiv preprints to one canonical shape: @misc with eprint /\n\
             # archiveprefix / url. Entries with a real venue are never touched.\n",
        );
        out.push_str(&format!("normalize_arxiv = {}\n", self.normalize_arxiv));
        out.push_str(
            "\n# Named profiles selectable with `normalize --profile <name>`. Each profile is a\n\
             # FULL config: any key it omits falls back to the built-in default (not to the\n\
             # base above). Uncomment to define one:\n\
             #\n\
             # [profiles.minimal]\n\
             # conference_acronyms = false\n\
             # protect_title_caps = false\n",
        );
        let mut names: Vec<&String> = self.profiles.keys().collect();
        names.sort();
        for name in names {
            let body = toml::to_string(&self.profiles[name]).unwrap_or_default();
            out.push_str(&format!("\n[profiles.{name}]\n{body}"));
        }
        out
    }
}

/// Fields kept; everything else is dropped. Two deliberate additions over the
/// spec's list: `crossref` (dropping it breaks BibTeX inheritance and defeats
/// export's crossref closure) and `editor` (`@incollection`/`@inbook`/
/// `@proceedings` need it).
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
    "crossref",
    "editor",
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
    // "conference on" is required so the *journal* "Language Resources and
    // Evaluation" (LREV) is never collapsed into the LREC conference.
    (r"conference on language resources and evaluation", "LREC"),
    (r"international conference on semantic computing", "ICSC"),
    // --- NLP: Asia-Pacific ---
    (
        r"asia-pacific chapter of the association for computational linguistics",
        "AACL",
    ),
    (
        r"international joint conference on natural language processing",
        "IJCNLP",
    ),
    // --- speech ---
    (
        r"international conference on acoustics,?\s*speech,?\s*and signal processing",
        "ICASSP",
    ),
    (
        r"international speech communication association|\binterspeech\b",
        "Interspeech",
    ),
    // --- AI (Europe) ---
    (r"european conference on artificial intelligence", "ECAI"),
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
    (
        "AACL",
        "Conference of the Asia-Pacific Chapter of the Association for Computational Linguistics (AACL)",
    ),
    (
        "IJCNLP",
        "International Joint Conference on Natural Language Processing (IJCNLP)",
    ),
    (
        "ICASSP",
        "IEEE International Conference on Acoustics, Speech and Signal Processing (ICASSP)",
    ),
    (
        "Interspeech",
        "Annual Conference of the International Speech Communication Association (Interspeech)",
    ),
    (
        "ECAI",
        "European Conference on Artificial Intelligence (ECAI)",
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
        // doi -> url (then drop the doi field). With the rule off, KEEP the
        // doi: it isn't on the whitelist, so falling through would delete it.
        if name == "doi" {
            if cfg.doi_to_url {
                doi_url = Some(doi_to_url(value));
            } else {
                out.set(name, value.clone());
            }
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
        // Identifier-ish fields carry `&` legitimately (query strings); every
        // prose field gets entities decoded and a bare `&` escaped for LaTeX.
        let new_value = if cfg.fix_entities && !matches!(name.as_str(), "url" | "eprint") {
            fix_entities(&new_value)
        } else {
            new_value
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
            notes.push("converted doi to url".to_string());
        } else {
            notes.push("dropped 'doi' (url already present)".to_string());
        }
    }

    if cfg.normalize_arxiv {
        arxiv_pass(&mut out, &mut notes);
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
    // A title carrying math is left alone: `strip_all_braces` + word-wise
    // `{{…}}` would split a `$…$` span across a brace group, which LaTeX
    // rejects ("Extra }, or forgotten $"). Conservative and idempotent.
    if value.contains('$') {
        return value.to_string();
    }
    // A title carrying a LaTeX command (`\emph{…}`, `{\'e}`, `\&`) keeps the
    // author's brace groups: flattening them would turn `\emph{foo}` into
    // `\emph foo`. `protect_capitals` copies existing groups verbatim and only
    // wraps bare capitalized words, so the result stays balanced + idempotent.
    let plain = if value.contains('\\') {
        value.to_string()
    } else {
        strip_all_braces(value)
    };
    let canon = fix_canonical_terms(&plain);
    protect_capitals(&canon)
}

fn normalize_booktitle(value: &str, anth_acro: Option<&str>, canonicalize: bool) -> String {
    if canonicalize {
        // An ACL Anthology id is authoritative and always collapses to canonical.
        if let Some(canon) = anth_acro.and_then(canonical_for_acronym) {
            return canon.to_string();
        }
        // Volume annotations strip BEFORE bare-acronym expansion (the spec's
        // order), so `ACL (Volume 1: Long Papers)` expands instead of
        // degrading to the bare string "ACL".
        let plain = strip_volume_annotation(&strip_all_braces(value));
        let plain = plain.trim();
        if let Some(expanded) = expand_bare_acronym(plain) {
            return expanded;
        }
        if let Some(canon) = canonical_venue(plain) {
            return canon;
        }
    }
    // Unrecognized, guarded, or canonicalization off: clean up, then tag the
    // acronym (append-only — the original name survives). No tag when the
    // string already carries the acronym, or declares a *different* venue's
    // acronym in parens (tagging VISAPP with "(ICCV)" would mislabel it).
    let mut inner = capitalise_content_words(&strip_volume_annotation(value));
    if let Some(acro) = anth_acro.or_else(|| get_acronym(&inner)) {
        if !already_has_acronym(&inner, acro)
            && !contains_word(&inner, acro)
            && !foreign_acronym_paren(&inner, acro)
        {
            inner = format!("{inner} ({acro})");
        }
    }
    inner
}

fn normalize_journal(value: &str, canonicalize: bool) -> String {
    if canonicalize {
        let plain = strip_all_braces(value);
        if let Some(expanded) = expand_bare_acronym(&plain) {
            return expanded;
        }
        if let Some(canon) = canonical_venue(&plain) {
            return canon;
        }
    }
    // Leave an unrecognized journal name alone, but tag a known acronym.
    let mut inner = value.to_string();
    if let Some(acro) = get_acronym(&inner) {
        if !already_has_acronym(&inner, acro)
            && !contains_word(&inner, acro)
            && !foreign_acronym_paren(&inner, acro)
        {
            inner = format!("{inner} ({acro})");
        }
    }
    inner
}

/// The canonical venue string for a recognized booktitle/journal, or `None` —
/// including when a guard (workshop/companion/joint signals, a foreign sibling
/// acronym, or a disjoint second venue in the same string) forbids replacing
/// the whole string. Guarded strings fall back to the append-only path.
fn canonical_venue(plain: &str) -> Option<String> {
    let acro = get_acronym(plain)?;
    if venue_guard_blocks_canonicalization(plain, acro) {
        return None;
    }
    canonical_for_acronym(acro).map(str::to_string)
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

fn compiled_rules() -> &'static [(Regex, &'static str)] {
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RES.get_or_init(|| {
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
    })
}

/// The text [`CONFERENCE_RULES`] match against: braces stripped, `&` in every
/// spelling (`\&`, `&amp;`, `&#38;`, bare) folded to " and ", whitespace
/// collapsed — so ACM's "Information & Knowledge Management" matches the
/// "and" wording the rules use. Matching-only; stored values are untouched.
fn acronym_haystack(field_value: &str) -> String {
    let folded = strip_all_braces(field_value)
        .replace("\\&", " and ")
        .replace("&amp;", " and ")
        .replace("&AMP;", " and ")
        .replace("&#38;", " and ")
        .replace('&', " and ");
    tidy(&folded)
}

fn get_acronym(field_value: &str) -> Option<&'static str> {
    let hay = acronym_haystack(field_value);
    compiled_rules()
        .iter()
        .find(|(re, _)| re.is_match(&hay))
        .map(|(_, a)| *a)
}

/// Words marking a satellite event whose booktitle must never be collapsed
/// into the main conference's canonical name.
const VENUE_GUARD_SIGNALS: &[&str] = &[
    "workshop",
    "companion",
    "tutorial",
    "doctoral",
    "shared task",
    "co-located",
    "colocated",
    "satellite",
];

/// Decision (b) guard: replacing the whole venue string is forbidden when the
/// string is a satellite event or a joint proceedings of two venues. Blocked
/// strings fall back to the append-only path (original name kept).
fn venue_guard_blocks_canonicalization(plain: &str, acro: &str) -> bool {
    let lower = plain.to_lowercase();
    let canonical_lower = canonical_for_acronym(acro)
        .map(str::to_lowercase)
        .unwrap_or_default();
    // A signal blocks unless it belongs to the venue's own canonical name
    // (SemEval and BlackboxNLP are themselves workshops).
    if VENUE_GUARD_SIGNALS
        .iter()
        .any(|s| lower.contains(s) && !canonical_lower.contains(s))
    {
        return true;
    }
    foreign_acronym_paren(plain, acro) || disjoint_second_venue(plain, acro)
}

/// A parenthesized sibling/joint acronym — `(VISAPP)`, `(EMNLP-IJCNLP)`,
/// `(LREC-COLING 2024)` — that isn't this venue's own. All-uppercase letters
/// only; `(ACL 2023)` and `(NAACL-HLT)` (the venue's own acronym leading) are
/// not foreign, and mixed-case names like `(SemEval)` never trigger.
fn foreign_acronym_paren(plain: &str, acro: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\(([^)]+)\)").unwrap());
    let own: String = acro
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    for c in re.captures_iter(plain) {
        let inner = c.get(1).unwrap().as_str();
        let letters: String = inner.chars().filter(|c| c.is_ascii_alphabetic()).collect();
        if letters.len() >= 2
            && letters.bytes().all(|b| b.is_ascii_uppercase())
            && !letters.starts_with(&own)
        {
            return true;
        }
    }
    false
}

/// A second, different venue spelled out disjointly in the same string — a
/// joint proceedings ("…EMNLP and the 9th IJCNLP…"). Overlapping rule matches
/// (NeurIPS vs its D&B track) are one venue, not a joint.
fn disjoint_second_venue(plain: &str, acro: &str) -> bool {
    let hay = acronym_haystack(plain);
    // The span of the rule that fired (first match overall, same as get_acronym).
    let Some(own_span) = compiled_rules()
        .iter()
        .find_map(|(re, _)| re.find(&hay).map(|m| (m.start(), m.end())))
    else {
        return false;
    };
    compiled_rules()
        .iter()
        .filter(|(_, a)| *a != acro)
        .filter_map(|(re, _)| re.find(&hay))
        .any(|m| m.end() <= own_span.0 || m.start() >= own_span.1)
}

/// Is `acro` already present as a standalone word ("…IJCAI Workshop…")? Then
/// the append-only fallback must not append it a second time.
fn contains_word(text: &str, acro: &str) -> bool {
    cached_regex(&format!(r"\b{}\b", regex::escape(acro))).is_match(text)
}

fn already_has_acronym(field_value: &str, acronym: &str) -> bool {
    let plain = strip_all_braces(field_value);
    cached_regex(&format!(r"\([^)]*{}[^)]*\)", regex::escape(acronym))).is_match(&plain)
}

/// Per-pattern regex cache for the handful of acronym-derived patterns built
/// at runtime — these run inside the per-entry loop, so compiling on every
/// call showed up on large libraries.
fn cached_regex(pattern: &str) -> Regex {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut map = cache.lock().unwrap_or_else(|p| p.into_inner());
    map.entry(pattern.to_string())
        .or_insert_with(|| Regex::new(pattern).unwrap())
        .clone()
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
    let stripped = re.replace_all(value, "").into_owned();
    if braces_balanced(&stripped) {
        stripped
    } else {
        // The regex ate an opening brace without its mate (a protected
        // annotation like `(Volume 1: {Long Papers})`). Retry on brace-free
        // text — the result must never corrupt the surrounding entry.
        re.replace_all(&strip_all_braces(value), "").into_owned()
    }
}

/// Same balance rule the serializer's gate ([`BibEntry::validate`]) enforces:
/// `{`/`}` balanced with no prefix dipping below zero.
fn braces_balanced(value: &str) -> bool {
    let mut depth: i32 = 0;
    for b in value.bytes() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Decode stray HTML entities (Crossref/ACM BibTeX carries them) and escape a
/// bare `&` to `\&` so the value is LaTeX-safe. `$…$` math runs are copied
/// verbatim; an existing `\&` is left alone (idempotent).
fn fix_entities(value: &str) -> String {
    // The `&`-family decodes to a fixpoint first ("&amp;amp;" → "&"), so the
    // escape below sees every ampersand exactly once.
    let mut decoded = value.to_string();
    loop {
        let next = decoded.replace("&amp;", "&").replace("&#38;", "&");
        if next == decoded {
            break;
        }
        decoded = next;
    }
    let decoded = decoded
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    let mut out = String::with_capacity(decoded.len() + 4);
    let mut in_math = false;
    let mut prev_backslash = false;
    for c in decoded.chars() {
        match c {
            '$' if !prev_backslash => {
                in_math = !in_math;
                out.push(c);
            }
            '&' if !in_math && !prev_backslash => out.push_str(r"\&"),
            _ => out.push(c),
        }
        prev_backslash = c == '\\' && !prev_backslash;
    }
    out
}

// ------------------------------------------------------------- arXiv pass
//
// The spec's step 3 (fix_bib's offline arXiv normalization): every known
// arXiv shape collapses to a canonical `@misc` with `eprint`,
// `archiveprefix = arXiv`, and an `arxiv.org/abs/` url. Entries with a real
// venue are never touched.

/// The spec's arXiv post-pass over an already-filtered entry.
fn arxiv_pass(out: &mut BibEntry, notes: &mut Vec<String>) {
    if is_published_venue_entry(out) {
        return;
    }
    if let Some(id) = detect_arxiv_id(out) {
        let before = out.clone();
        normalize_arxiv_entry(out, &id);
        if *out != before {
            notes.push("normalized arXiv preprint".to_string());
        }
    } else if out
        .get("journal")
        .is_some_and(|j| j.trim().eq_ignore_ascii_case("arxiv"))
    {
        // journal = {ArXiv} with no extractable id: still not a real venue.
        out.set_type("misc");
        out.remove("journal");
        notes.push("dropped arXiv pseudo-journal (no id found)".to_string());
    }
}

/// The arXiv id of an entry, in any of the spec's known shapes: a valid
/// `eprint` (excluding JSTOR), `journal = {ArXiv}` with the id in `url` or
/// `volume = {abs/…}`, or `journal = {arXiv:<id> [cs]}`.
fn detect_arxiv_id(e: &BibEntry) -> Option<String> {
    if e.get("eprinttype")
        .is_some_and(|t| t.to_lowercase().contains("jstor"))
    {
        return None;
    }
    static ID_NEW: OnceLock<Regex> = OnceLock::new();
    static ID_OLD: OnceLock<Regex> = OnceLock::new();
    let id_new = ID_NEW.get_or_init(|| Regex::new(r"^\d{4}\.\d{4,6}(v\d+)?$").unwrap());
    let id_old = ID_OLD.get_or_init(|| Regex::new(r"^[a-z-]+(\.[A-Z]{2})?/\d+$").unwrap());

    if let Some(eprint) = e.get("eprint") {
        let eid = eprint.trim();
        if id_new.is_match(eid) || id_old.is_match(eid) {
            return Some(eid.to_string());
        }
    }
    let journal = e.get("journal")?;
    let inner = journal.trim();
    if inner.eq_ignore_ascii_case("arxiv") {
        // journal = {ArXiv}: find the id elsewhere.
        if let Some(url) = e.get("url") {
            static URL_RE: OnceLock<Regex> = OnceLock::new();
            let re = URL_RE.get_or_init(|| Regex::new(r"arxiv\.org/abs/(\S+)").unwrap());
            if let Some(c) = re.captures(url) {
                return Some(c.get(1).unwrap().as_str().to_string());
            }
        }
        if let Some(volume) = e.get("volume") {
            static VOL_RE: OnceLock<Regex> = OnceLock::new();
            let re = VOL_RE.get_or_init(|| Regex::new(r"^abs/(\d{4}\.\d{4,6})").unwrap());
            if let Some(c) = re.captures(volume.trim()) {
                return Some(c.get(1).unwrap().as_str().to_string());
            }
        }
        return None;
    }
    // journal = {arXiv:2005.14165 [cs]}
    static J_RE: OnceLock<Regex> = OnceLock::new();
    let re = J_RE.get_or_init(|| Regex::new(r"^arXiv:(\d{4}\.\d{4,6})").unwrap());
    re.captures(inner)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}

/// True when the entry already names a real (non-arXiv) venue.
fn is_published_venue_entry(e: &BibEntry) -> bool {
    if e.get("booktitle").is_some() {
        return true;
    }
    if let Some(journal) = e.get("journal") {
        let inner = journal.trim().to_lowercase();
        if inner != "arxiv" && !inner.starts_with("arxiv:") {
            return true;
        }
    }
    false
}

/// Rewrite to the canonical arXiv shape (the caller has already checked for a
/// real venue): `@misc`, arXiv-style `journal`/`volume` dropped, canonical
/// `eprint` / `archiveprefix` / `url`.
fn normalize_arxiv_entry(e: &mut BibEntry, arxiv_id: &str) {
    if let Some(journal) = e.get("journal") {
        let inner = journal.trim().to_lowercase();
        if inner == "arxiv" || inner.starts_with("arxiv:") {
            e.remove("journal");
        }
    }
    if let Some(volume) = e.get("volume") {
        if volume.trim().starts_with("abs/") {
            e.remove("volume");
        }
    }
    e.set_type("misc");
    e.set("eprint", arxiv_id);
    e.set("archiveprefix", "arXiv");
    e.set("url", format!("https://arxiv.org/abs/{arxiv_id}"));
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
        once.validate()
            .expect("normalize produced an invalid entry");
        let (twice, notes) = normalize_entry(&once, &cfg);
        assert_eq!(once, twice);
        assert!(notes.is_empty(), "second pass changed something: {notes:?}");
    }

    #[test]
    fn doi_kept_when_doi_to_url_off() {
        let cfg = NormConfig {
            doi_to_url: false,
            ..NormConfig::default()
        };
        let e = BibEntry::new("article", "k")
            .with_field("title", "Hello")
            .with_field("doi", "10.1234/xyz");
        let (out, _) = normalize_entry(&e, &cfg);
        // With the rule off the doi must survive, not fall through the
        // keep-list and vanish.
        assert_eq!(out.get("doi"), Some("10.1234/xyz"));
        assert_eq!(out.get("url"), None);
    }

    #[test]
    fn doi_note_only_when_url_actually_added() {
        let e = BibEntry::new("article", "k")
            .with_field("title", "Hello")
            .with_field("doi", "10.1234/xyz")
            .with_field("url", "https://example.org/paper");
        let (out, notes) = normalize_entry(&e, &NormConfig::default());
        // The existing url wins; the notes must say what actually happened.
        assert_eq!(out.get("url"), Some("https://example.org/paper"));
        assert_eq!(out.get("doi"), None);
        assert!(
            notes.iter().any(|n| n.contains("dropped 'doi'")),
            "notes: {notes:?}"
        );
        assert!(
            !notes.iter().any(|n| n.contains("converted doi to url")),
            "notes claim a conversion that never happened: {notes:?}"
        );
    }

    #[test]
    fn keep_fields_matches_case_insensitively() {
        let mut cfg = NormConfig {
            keep_fields: vec!["Title".into(), "AUTHOR".into()],
            ..NormConfig::default()
        };
        cfg.normalize_self().unwrap();
        let e = BibEntry::new("article", "k")
            .with_field("title", "Hello")
            .with_field("author", "Doe, Jane")
            .with_field("year", "2024");
        let (out, _) = normalize_entry(&e, &cfg);
        assert_eq!(out.get("title"), Some("{{Hello}}"));
        assert!(out.get("author").is_some());
        assert_eq!(out.get("year"), None);
    }

    #[test]
    fn math_mode_titles_are_left_untouched() {
        // `{{$E}} = mc^2$` would split the math span across a brace group —
        // titles carrying math skip protection entirely.
        let e = BibEntry::new("article", "k").with_field("title", "Energy $E = mc^2$ Revisited");
        let cfg = NormConfig::default();
        let (once, _) = normalize_entry(&e, &cfg);
        assert_eq!(once.get("title"), Some("Energy $E = mc^2$ Revisited"));
        once.validate().unwrap();
        let (twice, notes) = normalize_entry(&once, &cfg);
        assert_eq!(once, twice);
        assert!(notes.is_empty());
    }

    #[test]
    fn entities_decoded_and_bare_ampersand_escaped() {
        assert_eq!(fix_entities("Foo &amp; Bar"), r"Foo \& Bar");
        assert_eq!(fix_entities("Foo & Bar"), r"Foo \& Bar");
        assert_eq!(fix_entities(r"Foo \& Bar"), r"Foo \& Bar"); // idempotent
        assert_eq!(fix_entities("&amp;amp;"), r"\&"); // double-encoded
        assert_eq!(fix_entities("$a & b$ and c & d"), r"$a & b$ and c \& d");
        // pipeline: a url keeps its query string verbatim
        let e = BibEntry::new("misc", "k")
            .with_field("title", "A & B")
            .with_field("url", "https://x.org/?a=1&b=2");
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        assert_eq!(out.get("url"), Some("https://x.org/?a=1&b=2"));
        assert_eq!(out.get("title"), Some(r"{{A}} \& {{B}}"));
    }

    #[test]
    fn malformed_norm_toml_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("norm.toml"), "keep_fields = [unclosed").unwrap();
        let err = NormConfig::load(dir.path()).unwrap_err();
        assert!(err.contains("norm.toml"), "err: {err}");
        // absent file still falls back to defaults
        let empty = tempfile::tempdir().unwrap();
        assert!(NormConfig::load(empty.path()).is_ok());
    }

    #[test]
    fn empty_keep_fields_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("norm.toml"), "keep_fields = []").unwrap();
        let err = NormConfig::load(dir.path()).unwrap_err();
        assert!(err.contains("keep_fields"), "err: {err}");
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
    fn documented_toml_round_trips_a_modified_config_with_profiles() {
        let mut cfg = NormConfig {
            max_authors: 3,
            canonicalize_venues: false,
            fix_entities: false,
            ..NormConfig::default()
        };
        cfg.profiles.insert(
            "strict".into(),
            NormConfig {
                protect_title_caps: false,
                ..NormConfig::default()
            },
        );
        let text = cfg.to_toml();
        let parsed: NormConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.max_authors, 3);
        assert!(!parsed.canonicalize_venues);
        assert!(!parsed.fix_entities);
        assert_eq!(parsed.keep_fields, cfg.keep_fields);
        assert!(!parsed.profiles["strict"].protect_title_caps);
        assert!(parsed.profiles["strict"].canonicalize_venues);
        // and it lands on disk via save(), loadable by load()
        let dir = tempfile::tempdir().unwrap();
        cfg.save(dir.path()).unwrap();
        let loaded = NormConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.max_authors, 3);
        assert!(loaded.profiles.contains_key("strict"));
    }

    #[test]
    fn set_option_parses_each_kind_and_rejects_unknown() {
        let mut cfg = NormConfig::default();
        cfg.set_option("fix_entities", "false").unwrap();
        assert!(!cfg.fix_entities);
        cfg.set_option("max_authors", "0").unwrap();
        assert_eq!(cfg.max_authors, 0);
        cfg.set_option("keep_fields", "Title, author,YEAR").unwrap();
        assert_eq!(cfg.keep_fields, vec!["title", "author", "year"]);
        assert!(cfg.set_option("keep_fields", " , ").is_err());
        assert!(cfg.set_option("max_authors", "many").is_err());
        assert!(cfg.set_option("nope", "true").is_err());
        assert!(cfg.set_option("doi_to_url", "maybe").is_err());
    }

    #[test]
    fn crossref_and_editor_survive_the_keep_list() {
        let e = BibEntry::new("incollection", "k")
            .with_field("title", "Chapter")
            .with_field("crossref", "book1")
            .with_field("editor", "Smith, Jane")
            .with_field("abstract", "noise");
        let (out, _) = normalize_entry(&e, &NormConfig::default());
        assert_eq!(out.get("crossref"), Some("book1"));
        assert_eq!(out.get("editor"), Some("Smith, Jane"));
        assert_eq!(out.get("abstract"), None);
    }

    #[test]
    fn latex_command_titles_keep_their_brace_groups() {
        let cfg = NormConfig::default();
        for (input, expected) in [
            (
                r"On \emph{Fast} Learning",
                r"{{On}} \emph{Fast} {{Learning}}",
            ),
            // the accent groups are copied verbatim; only the bare capital
            // before them is wrapped
            (r"R{\'e}sum{\'e} Parsing", r"{{R}}{\'e}sum{\'e} {{Parsing}}"),
        ] {
            let e = BibEntry::new("article", "k").with_field("title", input);
            let (once, _) = normalize_entry(&e, &cfg);
            assert_eq!(once.get("title"), Some(expected), "for {input:?}");
            once.validate().unwrap();
            let (twice, notes) = normalize_entry(&once, &cfg);
            assert_eq!(once, twice);
            assert!(notes.is_empty(), "{notes:?}");
        }
    }

    #[test]
    fn default_toml_matches_default_config() {
        let parsed: NormConfig = toml::from_str(&NormConfig::default().to_toml()).unwrap();
        let d = NormConfig::default();
        assert_eq!(parsed.keep_fields, d.keep_fields);
        assert_eq!(parsed.max_authors, d.max_authors);
        assert_eq!(parsed.conference_acronyms, d.conference_acronyms);
        assert_eq!(parsed.canonicalize_venues, d.canonicalize_venues);
        assert_eq!(parsed.doi_to_url, d.doi_to_url);
        assert_eq!(parsed.protect_title_caps, d.protect_title_caps);
        assert_eq!(parsed.tidy_whitespace, d.tidy_whitespace);
        assert_eq!(parsed.fix_entities, d.fix_entities);
        assert_eq!(parsed.normalize_arxiv, d.normalize_arxiv);
    }
}

# normalize — `niutero normalize`, `niutero rekey`, `niutero analyze`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** offline, **propose-only** normalization (`niutero-norm`) — drop noise
fields, tidy whitespace, protect title capitals, clip long author lists, and the
default **AI/ML venue canonicalization** ruleset; all idempotent. Default dry-run
shows a per-entry diff; `--write` applies; `--check` exits **2** if anything would
change (CI gate); `--json` emits the field-level `from→to` diffs. Config in
`.niutero/norm.toml` (defaults if absent).

**Venue canonicalization** (`canonicalize_venues`, on by default) collapses every
messy spelling of a recognized AI/ML conference or journal — the ordinal / year /
"Proceedings of the …" variants — to one canonical `Full Name (ACRONYM)` string:
e.g. "The Thirteenth International Conference on Learning Representations", a bare
"ICLR", and "Proc. of ICLR 2024" all become
"International Conference on Learning Representations (ICLR)". An ACL Anthology
DOI/URL is authoritative for the venue. Covers the major ML (NeurIPS, ICML, ICLR,
AISTATS, UAI, COLT, AAAI, IJCAI, …), NLP (ACL, EMNLP, NAACL, EACL, COLING, CoNLL,
Findings, SemEval, WMT, TACL, …), CV (CVPR, ICCV, ECCV, WACV, …), and IR/data
(SIGIR, KDD, CIKM, WWW, …) venues; non-AI venues (e.g. *Cognition*) are left
untouched. Each canonical string re-matches its own rule, so it is a fixed point
(re-normalizing is a no-op). Turn `canonicalize_venues` off to only append the
acronym to the cleaned original name.

**`rekey`** regenerates cite keys from a pattern mini-language
(`{auth}{year}{title.N}{Title.N}{title-content-word.N}`; casing follows the
token; default `{auth}{year}{title.1}{Title.2}` → `vaswani2017attentionIsAll`).
The pattern lives in `.niutero/config.toml` (`citekey_pattern`), or `--pattern`
overrides for one run. Preview by default, `--write` applies; collisions get a
deterministic `a`/`b`/… suffix; keys already matching the pattern are left
untouched (minimal churn), and the rename **migrates the `meta.json` sidecar**
(tags/notes/status/stars are keyed by cite key). The same generator auto-keys an
`add` that omits `--key`.

**`analyze`** is an offline health report: per-check counts (and, in `--json`,
the failing cite keys) for offline-changeable, odd titles, inconsistent venues
(spelling variants), missing url, and missing year.

Deferred: online enrichment (Semantic Scholar / DBLP / Crossref / OpenReview)
and the online "arXiv → published" / duplicate-merge health checks.

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

# normalize — `niutero normalize`, `niutero rekey`, `niutero analyze`

**Covers:** offline, **propose-only** normalization (`niutero-norm`) — drop noise
fields, tidy whitespace, protect title capitals, clip long author lists, decode
HTML entities and escape bare `&` (`fix_entities`), canonicalize arXiv preprints
(`normalize_arxiv`), and the default **AI/ML venue canonicalization** ruleset;
all idempotent. Default dry-run shows a per-entry diff; `--write` applies;
`--check` exits **2** if anything would change (CI gate); `--json` emits the
field-level `from→to` diffs. Config in `.niutero/norm.toml` (defaults if
absent; a **malformed file or an empty `keep_fields` is an error**, never a
silent fall-back to defaults — that would rewrite the library under the wrong
rules), with named `[profiles.<name>]` selectable via `--profile <name>` (each
profile is a full config, unspecified keys falling back to the built-in
defaults). `keep_fields` entries are lowercased on load.

**Write safety.** `normalize_apply` / `normalize_apply_keys` never write past
the serializer's gate: an entry whose proposal fails `BibEntry::validate()` is
kept as-is and reported as skipped (`NormReport.skipped`, a stderr warning in
the CLI) — a rule bug degrades a run, never corrupts the `.bib`. A library
already holding an invalid (hand-edited, tolerantly-parsed) entry refuses to
apply at all, since any write re-serializes every entry.

**The arXiv pass** (`normalize_arxiv`, on by default — the spec's step 3):
every known arXiv shape — `@misc` with `eprint`, `journal = {ArXiv}` with the
id in `url`/`volume = {abs/…}`, `journal = {arXiv:<id> [cs]}` — collapses to a
canonical `@misc` with `eprint` / `archiveprefix = {arXiv}` /
`url = {https://arxiv.org/abs/<id>}`; arXiv pseudo-journal/volume fields are
dropped. Entries with a real venue (a booktitle, or a non-arXiv journal) are
never touched; JSTOR eprints are excluded.

**Entities & ampersands** (`fix_entities`, on by default): stray HTML entities
(`&amp;` and friends — Crossref/ACM BibTeX ships them) are decoded and a bare
`&` is escaped to `\&` so values are LaTeX-safe. `$…$` math spans and the
identifier fields (`url`, `eprint`) are exempt. Venue *matching* additionally
folds every `&` spelling to " and " (`acronym_haystack`), so ACM's
"Information & Knowledge Management" still canonicalizes to CIKM. Titles
containing math (`$`) skip capital protection entirely — wrapping a `$…$` span
in `{{…}}` would break LaTeX.

**Venue canonicalization** (`canonicalize_venues`, on by default) collapses every
messy spelling of a recognized AI/ML conference or journal — the ordinal / year /
"Proceedings of the …" variants — to one canonical `Full Name (ACRONYM)` string:
e.g. "The Thirteenth International Conference on Learning Representations", a bare
"ICLR", and "Proc. of ICLR 2024" all become
"International Conference on Learning Representations (ICLR)". An ACL Anthology
DOI/URL is authoritative for the venue. Covers the major ML (NeurIPS, ICML, ICLR,
AISTATS, UAI, COLT, AAAI, IJCAI, ECAI, …), NLP (ACL, EMNLP, NAACL, EACL, AACL,
IJCNLP, COLING, CoNLL, Findings, SemEval, WMT, TACL, …), speech (ICASSP,
Interspeech), CV (CVPR, ICCV, ECCV, WACV, …), and IR/data (SIGIR, KDD, CIKM,
WWW, …) venues; non-AI venues (e.g. *Cognition*, the LREV journal) are left
untouched. Each canonical string re-matches its own rule, so it is a fixed point
(re-normalizing is a no-op).

**Guards** (`venue_guard_blocks_canonicalization`): a whole-string replacement
never fires on a satellite or joint event — a signal word
(workshop/companion/tutorial/doctoral/shared task/co-located/satellite, unless
it belongs to the venue's own canonical name, as with SemEval), a foreign
all-caps acronym in parens (`(VISAPP)`, `(EMNLP-IJCNLP)`), or a second venue
spelled out disjointly in the same string (a joint proceedings). Guarded
strings fall back to the append-only path: the original name survives, at most
gaining ` (ACRO)` — and not even that when the string already carries the
acronym or declares a different venue's. Volume annotations
(`(Volume 1: Long Papers)`) strip **before** bare-acronym expansion (spec
order) and the strip is brace-safe (an unbalanced result retries on brace-free
text — it can never corrupt the entry).

Turn `canonicalize_venues` off for append-only behavior everywhere: the
anthology override and bare-acronym expansion are gated too — the cleaned
original name is kept and only the acronym is appended.

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
(spelling variants), missing url, missing year, and likely duplicates
(`dedup::duplicate_groups`).

**Import-time normalization**: `run_import_hooks` (engine) is the one
post-import pipeline every front-end runs — enrich → normalize → PDF fetch over
`touched_keys()` (added + renamed + **overwritten**). `normalize_on_import`
gates it for bulk/CLI/GUI imports **and `add`/paste-BibTeX**; the connector
forces normalization regardless (a capture always lands clean). The automatic
paths normalize with `workflow.normalize_profile` (`config --normalize-profile
NAME`, `""` clears; validated against `norm.toml` when set) — `None` = the
base config; a manual `normalize --profile` still overrides per run.

**Editing the ruleset**: `norm-config <vault>` prints the options;
`--set key=value` (repeatable) changes one (`keep_fields` as a comma list,
`max_authors` as an integer, the rest `true`/`false`) through
`engine::set_norm_option` → `NormConfig::save`, which rewrites `norm.toml` in
its documented form (comments kept, profiles appended as tables). The GUI
Ruleset toggles are the same call, and are reseeded from `engine::norm_config`
whenever the tool's cache is rebuilt — `norm.toml` is the single source of
truth. `keep_fields` deliberately extends the spec's list with `crossref` and
`editor` (dropping them breaks BibTeX inheritance / `@incollection`).

**Titles with LaTeX commands** (`\emph{…}`, `{\'e}`, `\&`) keep their brace
groups: `strip_all_braces` is skipped for them, and `protect_capitals` copies
existing groups verbatim while wrapping only bare capitalized words — so
`R{\'e}sum{\'e} Parsing` → `{{R}}{\'e}sum{\'e} {{Parsing}}`, balanced and
idempotent.

Deferred: online enrichment (Semantic Scholar / DBLP / OpenReview) and the
online "arXiv → published" check (the *offline* arXiv canonicalization pass
exists — see above). Profile inheritance is from the built-in defaults, not
the base config (documented; changing it would alter existing vaults).

Tests: `niutero-norm/src/lib.rs` unit tests (incl.
`every_canonical_venue_is_a_fixed_point`,
`entities_decoded_and_bare_ampersand_escaped`, config loading);
`niutero-norm/tests/venues.rs` (canonicalization variants, guards
`workshop_and_joint_venues_are_never_whole_string_replaced`,
`cikm_ampersand_variants_canonicalize`,
`arxiv_preprint_normalizes_to_canonical_misc`, idempotence incl. the
`NIUTERO_BIB_FIXTURE` corpus harness — every pass also asserts `validate()`);
engine `normalize_*` tests (write-safety:
`normalize_refuses_to_write_over_an_invalid_entry`,
`a_proposal_that_fails_validation_is_skipped_not_recorded`; hooks:
`run_import_hooks_covers_touched_not_just_new`);
CLI `tests/normalize.rs` (dry-run/--write/--check,
`malformed_norm_toml_fails_with_exit_1`, `norm_config_shows_and_sets_options`,
`config_normalize_profile_selects_the_hooks_profile`), `tests/exchange.rs::
import_overwrite_runs_hooks_on_overwritten_keys`, `tests/mutate.rs::
add_normalizes_when_normalize_on_import_is_on`. Engine:
`normalize_profile_drives_the_automatic_paths`,
`norm_config_options_round_trip_through_norm_toml`; norm:
`documented_toml_round_trips_a_modified_config_with_profiles`,
`set_option_parses_each_kind_and_rejects_unknown`,
`latex_command_titles_keep_their_brace_groups`,
`crossref_and_editor_survive_the_keep_list`.

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

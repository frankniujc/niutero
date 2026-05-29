# normalize — `niutero normalize`, `niutero rekey`, `niutero analyze`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** offline, **propose-only** normalization (`niutero-norm`) — drop noise
fields, tidy whitespace, add `archiveprefix={arXiv}` when an `eprint` is present;
all idempotent. Default dry-run shows a per-entry diff; `--write` applies;
`--check` exits **2** if anything would change (CI gate); `--json` emits the
field-level `from→to` diffs. Config in `.niutero/norm.toml` (defaults if absent).

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

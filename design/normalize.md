# normalize — `niutero normalize`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** offline, **propose-only** normalization (`niutero-norm`) — drop noise
fields, tidy whitespace, add `archiveprefix={arXiv}` when an `eprint` is present;
all idempotent. Default dry-run shows a per-entry diff; `--write` applies;
`--check` exits **2** if anything would change (CI gate). Config in
`.niutero/norm.toml` (defaults if absent).

Deferred: online enrichment (Semantic Scholar / DBLP / Crossref / OpenReview).

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

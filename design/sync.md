# sync — `niutero connect`, `niutero sync`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** git synchronization via `niutero-sync` shelling out to the system
`git` (no libgit2, no credential handling) — `connect` (init + set `origin`) and
`sync` (commit → pull → push). A pull conflict aborts the merge and exits **2**
rather than leaving `<<<<<<<` markers.

Deferred: the structural per-entry 3-way conflict resolver (sync only aborts for
now).

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

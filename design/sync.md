# sync — `niutero connect`, `niutero sync`, `niutero history`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** git synchronization via `niutero-sync` shelling out to the system
`git` (no libgit2, no credential handling) — `connect` (init + set `origin`) and
`sync` (commit → pull → push).

On a pull conflict, `sync` attempts a **structured 3-way merge** of
`references.bib` (`niutero-core::merge`, entry- and field-level over the git
`:1:/:2:/:3:` stages): if the two sides touched different entries or different
*fields* of the same entry, it merges them, commits the merge, and continues
(reported as auto-merged). It stays conservative — it aborts (exit **2**, no
`<<<<<<<` markers) rather than guess whenever it can't prove the result correct:
a real same-field conflict, a whole-file modify/delete, a `@string`/verbatim
disagreement, or a conflict in any non-`references.bib` file (e.g. the sidecar).

Also `history <citekey>` (`--json`): the commits that changed one entry, newest
first. The entry's line span is located in the **committed `HEAD`** copy of
`references.bib` (git's `log -L` numbers lines against `HEAD`, and the span is
read from the actual committed bytes so a hand-formatted `.bib` still works),
then traced with `git log -L<start>,<end>:references.bib`. It is read-only and
informational — a missing repo / no commits / an entry not yet committed all
exit **1** with an actionable message; an entry deleted locally but still in the
last commit can still have its history shown.

Deferred: 3-way merge of the **sidecar** (tags/notes/status/stars in
`meta.json`) — a sidecar conflict currently aborts the whole sync.

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

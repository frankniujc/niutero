# sync — `niutero connect`, `niutero sync`, `niutero history`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** git synchronization via `niutero-sync` shelling out to the system
`git` (no libgit2, no credential handling) — `connect` (init + set `origin`) and
`sync` (commit → pull → push). A pull conflict aborts the merge and exits **2**
rather than leaving `<<<<<<<` markers.

Also `history <citekey>` (`--json`): the commits that changed one entry, newest
first. The entry's line span is located in the **committed `HEAD`** copy of
`references.bib` (git's `log -L` numbers lines against `HEAD`, and the span is
read from the actual committed bytes so a hand-formatted `.bib` still works),
then traced with `git log -L<start>,<end>:references.bib`. It is read-only and
informational — a missing repo / no commits / an entry not yet committed all
exit **1** with an actionable message; an entry deleted locally but still in the
last commit can still have its history shown.

Deferred: the structural per-entry 3-way conflict resolver (sync only aborts for
now).

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

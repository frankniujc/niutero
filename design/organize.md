# organize — `niutero tag`, `niutero note`, `niutero status`, `niutero stars`, `niutero view`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** the sidecar-only commands — `tag` (add/remove, sorted + deduped),
`note` (set/clear/show), `status` (unread/reading/done), `stars` (0–5; 0 clears),
and `view` (list/add/rm). These write only `.niutero/` (`meta.json` /
`views.toml`); `references.bib` is never touched. Empty meta entries are pruned,
and the workflow defaults (`unread` / unrated) are stored as *absent* so
`meta.json` stays minimal.

Tags are free-form strings; the namespaced convention (`topics:foo`, `wf:bar`)
is just a `:` in the name. The `list` / `export` filter understands `tag:foo`,
`status:reading`, and `stars:>=4` (also `>`, `<`, `<=`, `N`) terms, all ANDed.

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

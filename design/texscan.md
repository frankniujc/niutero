# texscan — `niutero tex-scan`

> **Status: planned.** Walkthrough not yet written — format pending review of
> [bib](bib.md) / [vault](vault.md) / [init](init.md). See [overview](overview.md).

**Covers:** the LaTeX citation audit — `core::texscan` scans `.tex`/`.aux` for
`\cite`/`\citep`/`\nocite`/`\citation` keys (options, comments, `\nocite{*}`),
reports `used` / `missing` / `unused`, exits **2** on undefined references (CI
gate), and `--out` writes a pruned `.bib` of just the cited entries
(`export_keys`).

<!-- Skeleton (see overview.md → Doc template):
## Command   ## What & why   ## Walkthrough   ## Output & exit codes
## Edge cases & errors   ## Tests   ## Deferred / gotchas -->

# export — `niutero export`, `niutero export-target`

**Covers:** writing a filtered subset to a standalone `.bib`, the safety guard
that protects the source of truth, the self-containedness rules (verbatim
blocks + crossref closure), and the keep-updated export-target family.

## Command

```sh
niutero-cli export <vault> --out FILE [--query Q | --view NAME] [--allow-empty] [--json]
niutero-cli export-target <vault> add FILE [--query Q]   # keep-updated mirror
niutero-cli export-target <vault> (list | rm FILE)
niutero-cli tex-scan <vault> <tex...> --out FILE          # pruned bibliography
```

## What & why

A `.bib` you hand to a co-author, an Overleaf checkout, or a paper build. The
export must be (a) **safe** — it can never damage the library it reads from —
and (b) **self-contained** — the file must compile under `bibtex` without the
rest of the library next to it.

## Walkthrough

`cmd_export` (`niutero-cli/src/main.rs`) → `engine::export(v, filter, out,
allow_empty)` (`niutero-engine/src/lib.rs`):

1. **Guard** — `resolve_export_out` (shared with `export_target_add`) resolves
   `out` to a stable absolute form and refuses the vault's own
   `references.bib` (case-insensitively — NTFS aliases included) and anything
   under `.niutero/`. Without this, `--out <vault>/references.bib` would
   atomically replace the library with the filtered subset.
2. **Lock** — `lock_vault`, so the read can't race a concurrent writer.
3. **Select** — the same `resolve_query` → `filter::entry_matches` path as
   `list` (sidecar facets joined via `facets_of`), or a fixed key set for
   `export_keys` (tex-scan's `--out`).
4. **`write_export`** (the shared tail for both paths):
   - `crossref_closure` — each selected entry's `crossref` target that exists
     in the library is appended (transitively) **after** the citing entries
     (BibTeX requires the parent to follow its children).
   - **Zero-match refusal** — an empty selection errors (exit 1) and writes
     nothing unless `--allow-empty`: a typo'd query must not blank the target.
   - **Validation** — every exported entry passes `BibEntry::validate()`; a
     tolerantly-parsed broken entry aborts the export naming the citekeys
     (the file goes to third parties — never hand over a corrupt one).
   - **Verbatim blocks** — every `@string` / `@preamble` / `@comment` block
     rides along, file order, before the entries, so abbreviations and
     `\newcommand` definitions still resolve. (Loose non-`@` prose between
     entries is left behind as noise.) Serialized with `to_bibtex`.
   - **Atomic write** — `niutero_vault::write_atomic` (temp + rename), the
     same crash-safety `references.bib` gets.

Entry ordering is library (file) order; entries serialize byte-identically to
a niutero-canonical `references.bib` (same `entry_block`).

## Keep-updated targets

`export-target add` registers a machine-local mirror (registry, not synced)
that `refresh_exports` re-exports after every mutation — the CLI calls it in
`refresh_keep_updated` after each mutating command, the connector after each
capture, and the GUI in `after_mutation`. Mirrors pass `allow_empty = true`
(they legitimately track whatever their filter matches), but `ExportOutcome.
emptied` flags a refresh that blanked a previously non-blank mirror, and every
host warns loudly (CLI stderr `mirror emptied`, GUI toast, connector log).

## Output & exit codes

Text: `Exported N entr(ies) to PATH`. `--json`: `{"exported": N, "keys": [...],
"out": PATH}` — the keys actually written (crossref parents included), so a
machine consumer can verify the file. `--out -` streams the `.bib` to stdout
(`export_to_string`, same rendering); it can't be combined with `--json`.
Exit `0` ok · `1` error (guard refusal, zero matches without `--allow-empty`,
invalid entries, unknown view, IO).

The GUI's Library toolbar has an **Export** button (`LibAction::ExportBib` →
`export_bib_flow`): it exports what the view currently shows — the active
sidebar tag plus the search box, both expressed in the same query language —
through `engine::export`, so every rule above applies.

## Edge cases & errors

- `--out <vault>/references.bib` (any casing) and `.niutero/…` → refused.
- Zero matches → error, target untouched; `--allow-empty` writes empty.
- Overwrite of an existing target file is unconditional (atomic).
- `--query` + `--view` together → error (`filter_from`).

## Tests

`niutero-cli/tests/exchange.rs`: `export_all_then_query_subset`,
`export_uses_saved_view`, `export_query_and_view_conflict`,
`export_refuses_the_vaults_own_references_bib`,
`export_refuses_paths_inside_niutero_dir`,
`export_zero_matches_errors_and_writes_nothing`,
`export_allow_empty_writes_an_empty_file`,
`export_includes_string_preamble_and_comment_blocks`,
`export_pulls_in_crossref_parents`,
`export_refuses_invalid_entries_and_names_them`,
`export_json_lists_the_written_keys`, `export_to_stdout_with_dash`.
`tests/texscan.rs`: `out_writes_pruned_bib`,
`tex_scan_out_includes_crossref_parents`.
`tests/organize.rs`: `tags_rename_refreshes_keep_updated_exports` (mirror
tracks + warns on emptying). `tests/registry.rs`: the `export-target` family.
Engine: `export_all_query_and_count`, `tex_scan_reports_and_export_keys_prunes`,
the `export_target_*` suite.

## Deferred / gotchas

- Keep-updated targets are registered from the CLI only (`export-target add`);
  the GUI refreshes them but has no add/remove UI.
- The GUI search box and the CLI query are the same language now
  (`engine::view_matches` wraps `filter::entry_matches`; free text also
  matches tags on both sides).

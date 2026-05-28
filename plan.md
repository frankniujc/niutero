# niutero — Phase 1 Plan (CLI-first rebuild)

## Why rebuild

The first build grew GUI-first: `niutero-app` is ~14,000 lines while `niutero-cli`
is ~730 and only exposes `normalize` / `sync` / `export` / `tex-scan`. All the base
operations — add, edit, delete, search, tags, views, import, init — live *only*
inside the egui app. The CLI could never be the real interface.

Phase 1 inverts the order: **build one complete, well-tested CLI for all base
functionality.** Phase 2's GUI is then a thin client over that same surface and can
do nothing the CLI can't.

## Principles

1. **CLI is the complete interface.** Every base capability is a subcommand. If the
   GUI will eventually need it, the CLI gets it first.
2. **Written from scratch.** No code is ported or referenced from the old
   `../niutero`. It informs the *product* only (data model, invariants, feature
   scope), never the implementation. Logic lives in pure, headless crates; the CLI is
   a thin wrapper over them.
3. **`.bib` is the source of truth and stays niutero-agnostic.** Private data
   (tags / notes / views) lives in `.niutero/`, never in the `.bib`.
4. **Deterministic, byte-stable serialization.** For an unchanged entry,
   `parse → serialize` is byte-identical. This is the #1 risk — lock it first.
5. **Test as we go, against a large real `.bib`.** No command ships without tests.
6. **Optional features stay off the base path** (sync, normalization, online enrich,
   import-by-DOI, LLM, PDF, browser connector).

## Scope — base functionality

Local, offline vault management. Must work with every optional feature off:

- **Vault**: init / open a folder (`references.bib` + `.niutero/`).
- **Entries**: add / show / edit / delete / list.
- **Search & filter**: free text + `tag:` queries.
- **Tags & notes**: stored in `.niutero/meta.json`.
- **Saved views**: named filters in `.niutero/views.toml`.
- **Import `.bib`**: merge an external file; duplicate policy (skip / overwrite / rename).
- **Export `.bib`**: filtered subset to a standalone file.
- **Config + machine registry** (`.niutero/config.toml`, machine-local `vaults.toml`).

Out of base scope — added later, once the base is locked: git sync, normalization
(offline rules + online enrich), import-by-DOI/identifier, LaTeX `tex-scan`, LLM
assist, PDF attachments, browser connector.

## Architecture

Fresh workspace in `niutero_2/`, written entirely from scratch — nothing is ported
from `../niutero`. A Cargo workspace that keeps logic out of the UI:

```
niutero-core    domain model (BibEntry, Library, config/meta/view types, filter) — no IO/UI
niutero-bib     tolerant .bib parser + deterministic serializer   <- the foundation
niutero-vault   vault IO: .niutero/ sidecar + machine-local registry
niutero-cli     the complete interface — a thin wrapper over the above
```

No GUI crate yet (that is Phase 2). The optional-feature crates
(sync / norm / fetch / import / pdf / llm / server) are added one at a time as later
milestones — also written fresh.

## CLI surface (base)

Every command takes `<vault>` as its first argument and supports `--json` for
machine-readable output (the contract Phase 2 / scripts consume).
Exit codes: `0` ok / `1` error / `2` actionable.

```
niutero init   <path>
niutero list   <vault> [--query Q | --view NAME] [--json]
niutero show   <vault> <citekey> [--json]
niutero add    <vault> (--bibtex STR | --from FILE | --type T --field k=v ...)
niutero edit   <vault> <citekey> (--field k=v ... | --unset k ...)
niutero rm     <vault> <citekey>
niutero tag    <vault> <citekey> [--add t ...] [--remove t ...]
niutero note   <vault> <citekey> (--set TEXT | --clear)
niutero view   <vault> (list | add NAME --query Q | rm NAME)
niutero import <vault> <file.bib> [--on-dup skip|overwrite|rename]
niutero export <vault> --out FILE [--query Q | --view NAME]
```

## Testing (this is the point of Phase 1)

- **Big-bib fixture.** Drop a large real library at
  `crates/niutero-bib/tests/fixtures/large.bib`. It is the workhorse for round-trip,
  import, and search tests.
- **Round-trip / determinism (highest priority).** Property test: `parse → serialize
  → parse` is stable; an unchanged entry re-serializes byte-for-byte. Run it over the
  big fixture, plus a golden snapshot. (`proptest` + `insta`.)
- **CLI integration tests.** Black-box each command with `assert_cmd` + `tempfile`:
  assert stdout (`--json`), exit code, and on-disk files.
- **Unit tests** live in each logic crate, written alongside the code.
- **CI gate:** `cargo test --workspace`, `cargo fmt --check`,
  `cargo clippy -D warnings`. Cross-platform matrix comes later.

## Milestones

- **M1 — Foundation.** Workspace scaffold; write `niutero-bib` + determinism tests
  against the big fixture; write `niutero-core` + `niutero-vault`; `init` + `list` +
  `show`. Locks the #1 risk.
- **M2 — Edit.** `add` / `edit` / `rm`; deterministic save; sidecar updates.
  Integration tests per command.
- **M3 — Organize.** Tags, notes, search/filter, saved views (`tag`, `note`,
  `list --query/--view`, `view`).
- **M4 — Exchange.** `import` (dup policy) + `export` (filtered). Round-trip
  import/export against the big fixture.
- **M5 — Round out.** Add `normalize` / `sync` / `tex-scan` through the CLI, each off
  the base path and tested.

## Phase 2 (later, not now)

GUI as a thin presentation layer over the same operations. Discipline: it builds on
the operations surface the CLI wraps (and consumes `--json` where it shells out) — it
never reaches under the CLI to do something the CLI cannot. Anything the GUI needs
becomes a CLI command first.

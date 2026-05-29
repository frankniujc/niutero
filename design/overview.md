# niutero — design walkthroughs

This folder holds step-by-step walkthroughs of every feature, kept in sync with
the code so we can **track & maintain** it. Each doc traces the real data flow
`CLI → engine → crate → disk`, names the actual functions, and points at the
tests that cover it — so when you change a feature you know exactly which doc and
which tests to touch.

Start here, then read the doc for whatever you're working on.

## Architecture

A Cargo workspace, logic split from UI. Dependency direction (arrows point at
the dependency):

```
niutero-cli ──► niutero-engine ──► niutero-vault ──► niutero-bib ──► niutero-core
                      │                                              ▲
                      ├──► niutero-sync (git shell-out)              │
                      └──► niutero-norm ────────────────────────────┘
```

| Crate | Owns |
|---|---|
| `niutero-core` | Domain model: `BibEntry`, `Library`, `filter`, `texscan`, `BibEntry::validate()`. No IO, no UI. |
| `niutero-bib` | Tolerant `.bib` parser + deterministic serializer over `BibItem` (`Entry` \| `Verbatim`). |
| `niutero-vault` | Vault IO: the folder layout, the `.niutero/` sidecar, **atomic writes**. |
| `niutero-sync` | git synchronization by shelling out to the system `git` (no libgit2, no credentials). |
| `niutero-norm` | Offline, propose-only normalization rules + `norm.toml` config. |
| `niutero-engine` | **The operations layer.** Every capability is a function here. |
| `niutero-cli` | A thin shell: parse args → call an engine op → format output. |

The point of `niutero-engine`: all behavior lives there, so the future GUI is a
thin client over the same code (it cannot do anything the CLI can't, and the two
can't drift).

## Data model — a library is a folder

```
<vault>/
├── references.bib      # the source of truth — portable, niutero-agnostic
└── .niutero/           # niutero's private sidecar (git-synced with the library)
    ├── config.toml     # Config { name, schema }
    ├── meta.json       # BTreeMap<citekey, { tags, note, added }>  (empty entries pruned)
    ├── views.toml      # Views { Vec<View { name, query }> }
    └── norm.toml       # optional normalization config (defaults if absent)
```

Organization is **tags + named saved views** — there is no "collection" object.

## The request pipeline

1. `niutero-cli/src/main.rs` — clap parses argv into a `Cmd`; a `cmd_*` handler
   builds an engine request (e.g. `Filter`, `AddSource`) and formats the result.
2. `niutero-engine/src/lib.rs` — the op (`add`, `list`, `import`, …) runs over an
   open `Vault`, calling core/bib/vault/sync/norm.
3. The crate does the pure work; `niutero-vault` performs the **atomic** disk write.

## Core invariants (do not violate)

1. **`.bib` is the source of truth and stays niutero-agnostic.** Tags/notes/views/
   config live only in `.niutero/`, never in `references.bib`.
2. **Deterministic, byte-stable, idempotent serialization.** An unchanged entry
   re-serializes byte-for-byte; the first save may canonicalize, then it's stable.
   This is the #1 risk — see [bib.md](bib.md).
3. **Validate untrusted input at the boundary.** The serializer assumes valid,
   brace-balanced entries; `BibEntry::validate()` is called before any write.
4. **Propose-only.** `normalize` never rewrites silently — it shows a diff;
   nothing changes without `--write`.
5. **Atomic writes.** `references.bib` and the sidecar are written via
   temp + fsync + rename, so a crash never truncates the source of truth.
6. **Cite keys are stable.** Phase 1 has no rename; `\cite{key}` must not break.
7. **Optional features stay off the base path.** The core works fully offline;
   sync/normalize add nothing the base depends on.
8. **The CLI is thin.** All operations live in `niutero-engine`.

## Exit codes

`0` = ok · `1` = error (bad usage / IO / not found) · `2` = **actionable** (a CI
gate: `tex-scan` undefined refs, `normalize --check` would-change, `sync`
conflict). Note: clap also exits `2` on argument-parse errors — see the deferred
note in [normalize.md](normalize.md) / [texscan.md](texscan.md).

## Testing strategy

- **Unit tests** in each crate (pure logic).
- **Round-trip**: `niutero-bib/tests/roundtrip.rs` (golden + idempotence + a large
  generated corpus + an optional `tests/fixtures/large.bib` hook) and
  `tests/proptest.rs` (adversarial value generators).
- **Black-box CLI**: `niutero-cli/tests/*.rs` run the real binary via `assert_cmd`,
  asserting stdout/stderr/exit code/on-disk files.
- **git integration**: `niutero-engine` tests drive a local bare-repo remote
  (push, fresh-clone-sees-it, two-clone conflict), skipped if `git` is absent.

Gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`.

## Index & status

| Doc | Covers | Status |
|---|---|---|
| [bib.md](bib.md) | `BibEntry`/`BibItem` model, parser, serializer, round-trip, `validate()` | written |
| [vault.md](vault.md) | folder layout, `.niutero/` sidecar IO, atomic writes | written |
| [init.md](init.md) | `init` | written |
| [browse.md](browse.md) | `list`, `show` | planned |
| [entries.md](entries.md) | `add`, `edit`, `rm` | planned |
| [organize.md](organize.md) | `tag`, `note`, `view` | planned |
| [import.md](import.md) | `import` | planned |
| [export.md](export.md) | `export` | planned |
| [texscan.md](texscan.md) | `tex-scan` | planned |
| [sync.md](sync.md) | `connect`, `sync` | planned |
| [normalize.md](normalize.md) | `normalize` | planned |

### Optional features — not yet built

In the original plan's "optional" list but **not implemented** (no walkthrough
yet). Tracked here so the gap stays visible:

- **PDF management & sync** — one PDF per entry, cached at
  `<vault>/pdfs/<citekey>.pdf` and synced to a private remote store (old niutero
  used a private HuggingFace dataset). The binary never goes in the `.bib`;
  `pdfs/` is auto-gitignored.
- **Import by DOI / identifier** — resolve a DOI / arXiv id over the network
  (today's `import` is file-only).
- **Online enrich** for `normalize` — Semantic Scholar / DBLP / Crossref.
- **LLM assist** — propose-only tag/search help; never edits the `.bib`.
- **Browser connector** — one-click capture from a web page via a loopback server.

## Doc template

Foundation docs (`bib`, `vault`) use a free architecture shape. Command docs
follow this skeleton, so they stay consistent and maintainable:

```
# <Title> — `command(s)`
## Command            — signature + flags
## What & why         — 1–2 sentences + the invariants it must honor
## Walkthrough        — numbered data-flow steps, with crate/file.rs:fn refs
## Output & exit codes — stdout / stderr (text + --json)
## Edge cases & errors
## Tests              — the covering test files / cases
## Deferred / gotchas — known gaps; what to watch when changing it
```

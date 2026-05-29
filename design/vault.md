# vault — folder layout and sidecar IO

Foundation doc. `niutero-vault` owns the on-disk shape of a library and all of
its IO. Every command opens a `Vault` and goes through here to read/write.

## A library is a folder

```
<vault>/
├── references.bib      # source of truth (niutero-bib parses/serializes it)
└── .niutero/
    ├── config.toml     # Config { name, schema }
    ├── meta.json       # BTreeMap<citekey, EntryMeta>  (per-entry private data)
    ├── views.toml      # Views { Vec<View { name, query }> }
    └── norm.toml       # read by niutero-norm; NOT written by this crate
```

Types (`niutero-vault/src/lib.rs`):

- `Config { name: String, schema: u32 }` — `SCHEMA_VERSION = 1`.
- `EntryMeta { tags: Vec<String>, note: String, added: Option<String> }`; empty
  fields are skipped on disk (`skip_serializing_if`) and `is_empty()` lets
  callers prune an entry that carries nothing.
- `Meta = BTreeMap<String, EntryMeta>` — keyed by cite key, **sorted** for stable
  diffs.
- `View { name, query }`, `Views { views: Vec<View> }`.
- `Vault { root, config, meta, views }` — an open vault: the path plus the loaded
  sidecar. Path helpers: `bib_path()`, `niutero_dir()`, and private
  `config_path()` / `meta_path()` / `views_path()`.

## init vs open

- **`Vault::init(root)`**: `create_dir_all`; error `AlreadyExists` if
  `.niutero/config.toml` already exists; build a `Vault` (name = folder name);
  write an empty `references.bib` **only if absent** (never clobber the source of
  truth); `save_sidecar()`. See [init.md](init.md) for the command path.
- **`Vault::open(root)`**: error if not a directory; load each sidecar file if
  present, else fall back to in-memory defaults. A plain folder (no `.niutero/`)
  opens with defaults and **nothing is written**, so read-only commands work on
  it.

## Reading & writing

- `read_items()` → `Vec<BibItem>` by parsing `references.bib` (empty vec if the
  file is absent).
- `write_items(items)` → serialize and write `references.bib` **atomically**.
- `save_sidecar()` → write `config.toml`, `meta.json` (pretty JSON + trailing
  newline), `views.toml`, each **atomically**. The three aren't transactional
  together, but each is individually crash-consistent.

## Atomic writes (`atomic_write`)

Every write goes to a uniquely-named temp file in the same directory, is
`write_all` + `sync_all`'d, then `fs::rename`'d over the target. `rename` is
atomic on one volume and replaces an existing file (including on Windows), so a
crash leaves either the old file or the new one — never a truncated
`references.bib`. On error the temp file is cleaned up (no `.tmp` litter).

## Invariants upheld here

- `.bib` stays niutero-agnostic — private data only ever goes to `.niutero/`.
- The source of truth is never left half-written (atomic).
- `init` never clobbers an existing `references.bib`.
- Deterministic, diff-friendly sidecar (`BTreeMap` order, `skip_serializing_if`,
  trailing newline).

## Tests

`niutero-vault/src/lib.rs` unit tests: `init_creates_layout`, `init_twice_errors`,
`init_does_not_clobber_existing_bib`, `sidecar_roundtrips`,
`bib_items_roundtrip_through_vault`, `open_plain_folder_uses_defaults`,
`save_sidecar_is_stable`, `atomic_writes_leave_no_temp_files`,
`write_items_overwrites_atomically`.

## Deferred / gotchas

- **`open()` masks a corrupt sidecar**: it falls back to defaults on *any* read
  error, not just `NotFound` — a transient/permission error could then be
  overwritten with empty defaults on the next save. (Deferred fix: match
  `NotFound` only; propagate other IO errors.)
- **No file locking** — two concurrent `niutero` invocations can lose each
  other's update (last writer wins).
- **`SCHEMA_VERSION` is written but never checked on open** — no version gate yet.
- `read_items` loads the whole `.bib` into memory and re-serializes the whole
  file on every mutation; fine for normal libraries, O(n) per edit for huge ones.
- `norm.toml` lives here but is owned by `niutero-norm` (see [normalize.md](normalize.md)).

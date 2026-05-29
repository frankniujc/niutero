# init — `niutero init`

## Command

```
niutero init <path>
```

Turn a folder into a niutero vault. The entry point for every other command.

## What & why

Creates the vault layout (`references.bib` + `.niutero/` sidecar) so the rest of
the CLI has something to open. Honors two invariants: the `.bib` stays the
source of truth (never clobbered), and writes are atomic.

## Walkthrough

1. **clap** parses `init <path>` into `Cmd::Init { path }`
   (`niutero-cli/src/main.rs`); `run` dispatches to `cmd_init(&path)`.
2. **`cmd_init`** (`main.rs`) calls `engine::init(path)`.
3. **`engine::init`** (`niutero-engine/src/lib.rs`) wraps `Vault::init(path)`,
   mapping any error to `init <path>: <e>`.
4. **`Vault::init`** (`niutero-vault/src/lib.rs`):
   - `create_dir_all(root)` (creates missing parent dirs too);
   - if `.niutero/config.toml` already exists → `AlreadyExists` error
     `"<path> is already a niutero vault"`;
   - build the `Vault` with `name = <folder name>`, empty `meta`/`views`;
   - write an empty `references.bib` **only if absent** (`atomic_write`);
   - `save_sidecar()` → `config.toml` (`name`, `schema = 1`), `meta.json` (`{}`),
     `views.toml`, each written atomically.
5. Back in `cmd_init`: prints `Initialized vault '<name>' at <path>`.

## Output & exit codes

- Success → stdout `Initialized vault '<name>' at <path>`, exit `0`.
- Already a vault → stderr `error: init <path>: <path> is already a niutero vault`,
  exit `1`.
- (No `--json` — `init` is a one-shot setup command.)

## Edge cases & errors

- Missing parent directories are created.
- An **existing `references.bib` is preserved** — `init` only adds the
  `.niutero/` sidecar around it. So pointing `init` at a folder that already has
  a `.bib` adopts it as the library.
- Re-running `init` on an initialized vault is a no-op error (exit 1), not a
  clobber.

## Tests

- `niutero-cli/tests/cli.rs`: `init_creates_vault`, `init_existing_errors`.
- `niutero-vault/src/lib.rs`: `init_creates_layout`, `init_twice_errors`,
  `init_does_not_clobber_existing_bib`.

## Deferred / gotchas

- **Does not scaffold a README** — neither a vault-level `README.md` explaining
  the library to collaborators, nor (separately) a top-level `README.md` for the
  niutero_2 repo itself. Flagged 2026-05-29, deferred.
- **Does not write `.niutero/norm.toml`** — normalization config is currently
  "defaults if absent" and therefore invisible; `init` should drop a documented
  default so the knobs are discoverable. Deferred (both write only-if-absent).
- Minor TOCTOU between the `exists()` guard and the write (could use
  `create_new`); low risk.

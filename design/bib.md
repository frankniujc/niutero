# bib — entries and the `.bib` format

## What this does

Your bibliography is a plain-text `.bib` file. This layer does two things: it
**reads** that text into structured entries, and **writes** entries back out —
in one fixed layout, **byte-for-byte the same every time**.

That second part is the whole point. Because saving an unchanged entry always
produces the exact same bytes, editing one reference shows up as one line in a
git diff — no churn, no merge noise. This is the foundation the rest of niutero
stands on; if it's wrong, collaborating on a library becomes miserable.

## The pieces

```
.bib text ──parse──► entries ──(edit)──► entries ──serialize──► .bib text
                                              │
                                          validate   (gate before any write)
```

- **Model** — what an entry looks like in memory.
- **Parser** — text → entries; tolerant, never crashes on messy input.
- **Serializer** — entries → text; one canonical layout, byte-stable.
- **Validation** — refuses an entry that would corrupt the file.

## Details

### The model

An entry (`BibEntry`, in `niutero-core`) is a cite key, a type, and an **ordered**
list of fields:

```
@inproceedings{niu2025,
  title = {…},
  year  = {2025}
}
```

Field order is preserved — that's what keeps saves byte-stable. Types and field
names are lowercased; values are kept exactly as written. A whole file is a list
of entries plus any `@string` / `@preamble` / `@comment` blocks, which ride
through untouched.

### Parsing (`niutero-bib/src/parse.rs`)

`parse(text)` is a tolerant scanner: it pulls out entries and keeps anything it
doesn't understand verbatim instead of dropping it. Braces and quotes inside a
value are tracked, so a `,` or `}` inside `{…}` doesn't end the field early.

### Serializing (`niutero-bib/src/serialize.rs`)

`to_bibtex(...)` emits one canonical form: `@type{key,` then one `name = {value}`
per line, two-space indent, no trailing comma. Same entry in → same bytes out.

The contract is **canonicalize-on-write, then idempotent**: the *first* save of a
messy file tidies it (lowercasing, re-indenting, `"x"`→`{x}`); every save after
is identical. The tests pin that fixed point.

### Validation (`BibEntry::validate`)

The serializer trusts its input, so entries from untrusted sources (CLI args, an
imported file) are checked first — legal cite key, sane type and field names, and
**balanced braces** in values (an unbalanced `x}` would otherwise break the
file). Enforced at the `add` / `edit` / `import` boundary.

## Tests

- `niutero-bib/tests/roundtrip.rs` — golden output, idempotence on a tricky
  fixture and a large generated corpus (plus an optional `tests/fixtures/large.bib`).
- `niutero-bib/tests/proptest.rs` — generated entries with adversarial values.
- unit tests in `parse.rs` / `serialize.rs`, and `validate()` tests in core.

## Deferred / gotchas

- CRLF / BOM aren't normalized on read — a CRLF file re-saves as LF once.
- Bare macros (`month = jan`) and `#`-concatenations become literals on first save.

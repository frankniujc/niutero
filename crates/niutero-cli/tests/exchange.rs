//! Black-box tests for M4: import (with duplicate policy) and export.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn niutero() -> Command {
    isolate_registry();
    Command::cargo_bin("niutero-cli").expect("binary built")
}

/// Point the machine-local registry at a per-binary temp file (inherited by the
/// spawned process) so tests never touch — or race on — the real machine one.
fn isolate_registry() {
    use std::sync::OnceLock;
    static REG: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = REG.get_or_init(|| tempfile::tempdir().expect("registry tempdir"));
    std::env::set_var("NIUTERO_REGISTRY", dir.path().join("vaults.toml"));
}

fn vault_with(contents: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    fs::write(d.path().join("references.bib"), contents).unwrap();
    d
}

fn bib(d: &TempDir) -> String {
    fs::read_to_string(d.path().join("references.bib")).unwrap()
}

fn import_file(d: &TempDir, contents: &str) -> PathBuf {
    let p = d.path().join("incoming.bib");
    fs::write(&p, contents).unwrap();
    p
}

// ------------------------------------------------------------------ import

#[test]
fn import_default_skips_duplicates() {
    let d = vault_with("@misc{k,\n  title = {Orig}\n}\n");
    let f = import_file(&d, "@misc{k, title={New}}\n@misc{fresh, title={F}}\n");
    niutero()
        .arg("import")
        .arg(d.path())
        .arg(&f)
        .assert()
        .success()
        .stdout(predicate::str::contains("1 added"))
        .stdout(predicate::str::contains("1 skipped"));
    let s = bib(&d);
    assert!(s.contains("title = {Orig}"), "existing entry untouched");
    assert!(s.contains("@misc{fresh,"));
}

#[test]
fn import_overwrite() {
    let d = vault_with("@misc{k,\n  title = {Orig}\n}\n");
    let f = import_file(&d, "@article{k, title={New}}\n");
    niutero()
        .arg("import")
        .arg(d.path())
        .arg(&f)
        .args(["--on-dup", "overwrite"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 overwritten"));
    let s = bib(&d);
    assert!(s.contains("@article{k,"));
    assert!(s.contains("title = {New}"));
    assert!(!s.contains("Orig"));
}

#[test]
fn import_rename_keeps_both() {
    let d = vault_with("@misc{k,\n  title = {Orig}\n}\n");
    let f = import_file(&d, "@misc{k, title={New}}\n");
    niutero()
        .arg("import")
        .arg(d.path())
        .arg(&f)
        .args(["--on-dup", "rename"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 renamed"))
        .stdout(predicate::str::contains("k -> k-2"));
    let s = bib(&d);
    assert!(s.contains("@misc{k,"));
    assert!(s.contains("@misc{k-2,"));
}

#[test]
fn import_overwrite_runs_hooks_on_overwritten_keys() {
    // With normalize-on-import on, an overwritten entry must be re-cleaned
    // exactly like a fresh add (hooks run over touched keys, not new keys).
    let d = vault_with("@article{k,\n  title = {Orig}\n}\n");
    niutero()
        .arg("config")
        .arg(d.path())
        .args(["--normalize-on-import", "true"])
        .assert()
        .success();
    let f = import_file(&d, "@article{k, title={New  Title}, abstract={x}}\n");
    niutero()
        .arg("import")
        .arg(d.path())
        .arg(&f)
        .args(["--on-dup", "overwrite"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 overwritten"))
        .stderr(predicate::str::contains("normalized 1"));
    let s = bib(&d);
    assert!(
        !s.contains("abstract"),
        "overwritten entry not normalized: {s}"
    );
    assert!(s.contains("{{New}} {{Title}}"), "got: {s}");
}

#[test]
fn import_no_entries_errors() {
    let d = vault_with("");
    let f = import_file(&d, "nothing to see here\n");
    niutero()
        .arg("import")
        .arg(d.path())
        .arg(&f)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no BibTeX entries"));
}

// ------------------------------------------------------------------ export

#[test]
fn export_all_then_query_subset() {
    let d = vault_with("@article{a, title={Apple}}\n@misc{b, title={Banana}}\n");
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported 2"));

    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--query", "apple"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported 1"));
    let written = fs::read_to_string(&out).unwrap();
    assert!(written.contains("@article{a,"));
    assert!(!written.contains("@misc{b,"));
}

#[test]
fn export_uses_saved_view() {
    let d = vault_with("@misc{a, title={Apple}}\n@misc{b, title={Banana}}\n");
    niutero()
        .arg("view")
        .arg(d.path())
        .arg("add")
        .arg("apples")
        .args(["--query", "apple"])
        .assert()
        .success();
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--view", "apples"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported 1"));
    let written = fs::read_to_string(&out).unwrap();
    assert!(written.contains("Apple"));
    assert!(!written.contains("Banana"));
}

#[test]
fn export_query_and_view_conflict() {
    let d = vault_with("@misc{k}\n");
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--query", "x", "--view", "v"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("either --query or --view"));
}

#[test]
fn export_json_lists_the_written_keys() {
    let d = vault_with("@article{a, title={Apple}}\n@misc{b, title={Banana}}\n");
    let out = d.path().join("out.bib");
    let stdout = niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--query", "apple", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(v["exported"], serde_json::json!(1));
    assert_eq!(v["keys"], serde_json::json!(["a"]));
}

#[test]
fn export_to_stdout_with_dash() {
    let d = vault_with("@article{a, title={Apple}}\n@misc{b, title={Banana}}\n");
    let stdout = niutero()
        .arg("export")
        .arg(d.path())
        .args(["--out", "-", "--query", "apple"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(stdout).unwrap();
    assert!(text.contains("@article{a,"), "got: {text}");
    assert!(!text.contains("Banana"), "got: {text}");
    assert!(!d.path().join("-").exists(), "no file named '-' may appear");
    // stdout can't carry both the .bib and the --json report
    niutero()
        .arg("export")
        .arg(d.path())
        .args(["--out", "-", "--json"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn export_refuses_the_vaults_own_references_bib() {
    // `--out <vault>/references.bib` would atomically REPLACE the library with
    // the filtered subset (0 bytes on a typo'd query). Must refuse, exit 1,
    // library byte-identical — including a casing alias on NTFS.
    let d = vault_with("@misc{k, title={Keep Me}}\n");
    let before = bib(&d);
    for name in ["references.bib", "References.bib"] {
        niutero()
            .arg("export")
            .arg(d.path())
            .arg("--out")
            .arg(d.path().join(name))
            .args(["--query", "nomatch"])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("references.bib"));
        assert_eq!(bib(&d), before, "library must be untouched ({name})");
    }
}

#[test]
fn export_refuses_paths_inside_niutero_dir() {
    let d = vault_with("@misc{k, title={T}}\n");
    let target = d.path().join(".niutero").join("meta.json");
    let before = fs::read_to_string(&target).ok();
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&target)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(".niutero"));
    assert_eq!(fs::read_to_string(&target).ok(), before);
}

#[test]
fn export_zero_matches_errors_and_writes_nothing() {
    let d = vault_with("@misc{k, title={Apple}}\n");
    let out = d.path().join("out.bib");
    // Pre-existing good export: a typo'd query must not blank it.
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let good = fs::read_to_string(&out).unwrap();
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--query", "tag:thesus"]) // the classic typo
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("matched no entries"));
    assert_eq!(fs::read_to_string(&out).unwrap(), good, "target untouched");
}

#[test]
fn export_allow_empty_writes_an_empty_file() {
    let d = vault_with("@misc{k, title={Apple}}\n");
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--query", "nomatch", "--allow-empty"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported 0"));
    assert_eq!(fs::read_to_string(&out).unwrap().trim(), "");
}

#[test]
fn export_includes_string_preamble_and_comment_blocks() {
    // @string refs and @preamble \newcommand definitions must survive an
    // export, or the exported file silently changes meaning / breaks LaTeX.
    let d = vault_with(
        "@string{acl = {Association for Computational Linguistics}}\n\n\
         @preamble{ \"\\newcommand{\\noopsort}[1]{}\" }\n\n\
         @comment{ tool metadata }\n\n\
         @misc{k, title={Apple}, publisher = {ACL Press}}\n",
    );
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let w = fs::read_to_string(&out).unwrap();
    assert!(w.contains("@string{acl"), "got: {w}");
    assert!(w.contains("@preamble"), "got: {w}");
    assert!(w.contains("@comment"), "got: {w}");
    let entry_pos = w.find("@misc{k").expect("entry present");
    assert!(
        w.find("@string").unwrap() < entry_pos,
        "verbatim blocks come first: {w}"
    );
}

#[test]
fn export_pulls_in_crossref_parents() {
    // BibTeX requires a crossref'd parent to be present (after the child), or
    // the exported file is broken for anyone we hand it to.
    let d = vault_with(
        "@inproceedings{paper, title={Apple Study}, crossref={proc}}\n\n\
         @proceedings{proc, booktitle={Some Proceedings}, year={2020}}\n",
    );
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .args(["--query", "apple"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported 2"));
    let w = fs::read_to_string(&out).unwrap();
    let child = w.find("@inproceedings{paper").expect("child present");
    let parent = w.find("@proceedings{proc").expect("parent pulled in");
    assert!(child < parent, "parent must follow the child: {w}");
}

#[test]
fn export_refuses_invalid_entries_and_names_them() {
    // A tolerantly-parsed broken entry must never reach a file we hand to a
    // third party — refuse and name the culprit.
    let d = vault_with("@misc{good, title={Fine}}\n\n@misc{bad, title = {Hello\n");
    let out = d.path().join("out.bib");
    niutero()
        .arg("export")
        .arg(d.path())
        .arg("--out")
        .arg(&out)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("bad"));
    assert!(!out.exists(), "nothing may be written");
}

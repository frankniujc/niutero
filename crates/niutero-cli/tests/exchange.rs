//! Black-box tests for M4: import (with duplicate policy) and export.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn niutero() -> Command {
    Command::cargo_bin("niutero").expect("binary built")
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

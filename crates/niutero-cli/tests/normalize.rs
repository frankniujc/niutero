//! Black-box tests for `normalize` (M5): propose-only dry run, --write,
//! and the --check CI gate (exit 2).

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
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

const DIRTY: &str = "@article{k,\n  title = {A  B},\n  abstract = {x}\n}\n";

#[test]
fn dry_run_reports_without_writing() {
    let d = vault_with(DIRTY);
    let before = bib(&d);
    niutero()
        .arg("normalize")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("would change"))
        .stdout(predicate::str::contains("dropped 'abstract'"))
        .stdout(predicate::str::contains("rewrote 'title'"));
    assert_eq!(bib(&d), before, "dry run must not write");
}

#[test]
fn write_applies_and_is_idempotent() {
    let d = vault_with(DIRTY);
    niutero()
        .arg("normalize")
        .arg(d.path())
        .arg("--write")
        .assert()
        .success()
        .stdout(predicate::str::contains("changed"));
    let s = bib(&d);
    assert!(!s.contains("abstract"));
    // capitalized words {{}}-protected; serializer adds one outer brace pair
    assert!(s.contains("title = {{{A}} {{B}}}"), "got: {s}");
    niutero()
        .arg("normalize")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Already normalized"));
}

#[test]
fn check_exits_two_when_dirty_zero_when_clean() {
    let d = vault_with(DIRTY);
    niutero()
        .arg("normalize")
        .arg(d.path())
        .arg("--check")
        .assert()
        .code(2);
    assert!(bib(&d).contains("abstract"), "--check must not write");

    niutero()
        .arg("normalize")
        .arg(d.path())
        .arg("--write")
        .assert()
        .success();
    niutero()
        .arg("normalize")
        .arg(d.path())
        .arg("--check")
        .assert()
        .success();
}

#[test]
fn write_and_check_are_mutually_exclusive() {
    let d = vault_with(DIRTY);
    niutero()
        .arg("normalize")
        .arg(d.path())
        .args(["--write", "--check"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("either --write or --check"));
}

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
fn malformed_norm_toml_fails_with_exit_1() {
    // A config typo must fail loudly — never silently revert every rule to
    // its default and rewrite the library.
    let d = vault_with(DIRTY);
    let before = bib(&d);
    fs::write(
        d.path().join(".niutero").join("norm.toml"),
        "keep_fields = [unclosed",
    )
    .unwrap();
    niutero()
        .arg("normalize")
        .arg(d.path())
        .arg("--write")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("norm.toml"));
    assert_eq!(bib(&d), before, "nothing may be written on a config error");
}

#[test]
fn norm_config_shows_and_sets_options() {
    let d = vault_with(DIRTY);
    niutero()
        .arg("norm-config")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fix_entities:        true"));
    niutero()
        .arg("norm-config")
        .arg(d.path())
        .args([
            "--set",
            "protect_title_caps=false",
            "--set",
            "max_authors=3",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("protect_title_caps:  false"))
        .stdout(predicate::str::contains("max_authors:         3"));
    // persisted: the next normalize no longer brace-protects the title
    niutero()
        .arg("normalize")
        .arg(d.path())
        .arg("--write")
        .assert()
        .success();
    let s = bib(&d);
    assert!(s.contains("title = {A B}"), "got: {s}");
    // --json shape + a bad key errors
    let out = niutero()
        .arg("norm-config")
        .arg(d.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["max_authors"], serde_json::json!(3));
    assert_eq!(v["protect_title_caps"], serde_json::json!(false));
    niutero()
        .arg("norm-config")
        .arg(d.path())
        .args(["--set", "nope=true"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown normalize option"));
}

#[test]
fn config_normalize_profile_selects_the_hooks_profile() {
    let d = vault_with(DIRTY);
    fs::write(
        d.path().join(".niutero").join("norm.toml"),
        "[profiles.keepall]\nkeep_fields = [\"title\", \"abstract\"]\nprotect_title_caps = false\n",
    )
    .unwrap();
    // unknown profile is refused up front
    niutero()
        .arg("config")
        .arg(d.path())
        .args(["--normalize-profile", "nope"])
        .assert()
        .failure()
        .code(1);
    niutero()
        .arg("config")
        .arg(d.path())
        .args([
            "--normalize-profile",
            "keepall",
            "--normalize-on-import",
            "true",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("normalize profile:   keepall"));
    // the hook now normalizes with the profile: abstract kept, title untouched
    niutero()
        .arg("add")
        .arg(d.path())
        .args([
            "--bibtex",
            "@article{p, title={Keep Case}, abstract={kept}, month={jan}}",
        ])
        .assert()
        .success();
    let s = bib(&d);
    assert!(s.contains("abstract = {kept}"), "got: {s}");
    assert!(s.contains("title = {Keep Case}"), "got: {s}");
    assert!(!s.contains("month"), "got: {s}");
    // "" clears it
    niutero()
        .arg("config")
        .arg(d.path())
        .args(["--normalize-profile", ""])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "normalize profile:   (base config)",
        ));
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

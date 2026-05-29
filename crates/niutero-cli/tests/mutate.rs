//! Black-box tests for the mutating commands: add / edit / rm. Each asserts on
//! the exact canonical `references.bib` so determinism is pinned, plus error
//! paths and sidecar cleanup.

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

fn new_vault() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    d
}

fn set_bib(d: &TempDir, contents: &str) {
    fs::write(d.path().join("references.bib"), contents).unwrap();
}

fn bib(d: &TempDir) -> String {
    fs::read_to_string(d.path().join("references.bib")).unwrap()
}

// ---------------------------------------------------------------- add

#[test]
fn add_from_flags_writes_canonical() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args([
            "--type",
            "article",
            "--key",
            "foo",
            "--field",
            "title=Hello World",
            "--field",
            "year=2020",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 1: foo"));
    assert_eq!(
        bib(&d),
        "@article{foo,\n  title = {Hello World},\n  year = {2020}\n}\n"
    );
}

#[test]
fn add_from_bibtex_is_canonicalized() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--bibtex", "@MISC{bar, Title = {X}, note = \"hi\"}"])
        .assert()
        .success();
    // type + field names lowercased, quoted value de-quoted to braces
    assert_eq!(bib(&d), "@misc{bar,\n  title = {X},\n  note = {hi}\n}\n");
}

#[test]
fn add_from_file_adds_all() {
    let d = new_vault();
    let f = d.path().join("in.bib");
    fs::write(&f, "@book{b1, title={T1}}\n@book{b2, title={T2}}\n").unwrap();
    niutero()
        .arg("add")
        .arg(d.path())
        .arg("--from")
        .arg(&f)
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 2"));
    let s = bib(&d);
    assert!(s.contains("@book{b1,"));
    assert!(s.contains("@book{b2,"));
}

#[test]
fn add_appends_after_existing_and_keeps_verbatim() {
    let d = new_vault();
    set_bib(
        &d,
        "@string{acl = {ACL}}\n\n@misc{keep,\n  title = {K}\n}\n",
    );
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "new1"])
        .assert()
        .success();
    assert_eq!(
        bib(&d),
        "@string{acl = {ACL}}\n\n@misc{keep,\n  title = {K}\n}\n\n@misc{new1\n}\n"
    );
}

#[test]
fn add_duplicate_errors() {
    let d = new_vault();
    set_bib(&d, "@misc{dup,\n  title = {x}\n}\n");
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "dup"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn add_without_a_mode_errors() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("specify --bibtex, --from, or"));
}

// ---------------------------------------------------------------- edit

#[test]
fn edit_set_unset_and_type() {
    let d = new_vault();
    set_bib(&d, "@article{e1,\n  title = {Old},\n  year = {1999}\n}\n");
    niutero()
        .arg("edit")
        .arg(d.path())
        .arg("e1")
        .args([
            "--field",
            "title=New Title",
            "--unset",
            "year",
            "--type",
            "inproceedings",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated e1"));
    assert_eq!(bib(&d), "@inproceedings{e1,\n  title = {New Title}\n}\n");
}

#[test]
fn edit_missing_citekey_errors() {
    let d = new_vault();
    niutero()
        .arg("edit")
        .arg(d.path())
        .arg("nope")
        .args(["--field", "x=1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no entry with cite key 'nope'"));
}

#[test]
fn edit_without_changes_errors() {
    let d = new_vault();
    set_bib(&d, "@misc{e2\n}\n");
    niutero()
        .arg("edit")
        .arg(d.path())
        .arg("e2")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("at least one of"));
}

// ---------------------------------------------------------------- rm

#[test]
fn rm_removes_entry_and_cleans_meta() {
    let d = new_vault();
    set_bib(
        &d,
        "@misc{a,\n  title = {A}\n}\n\n@misc{b,\n  title = {B}\n}\n",
    );
    fs::write(
        d.path().join(".niutero").join("meta.json"),
        "{ \"a\": { \"tags\": [\"x\"] } }",
    )
    .unwrap();
    niutero()
        .arg("rm")
        .arg(d.path())
        .arg("a")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed a"));
    assert_eq!(bib(&d), "@misc{b,\n  title = {B}\n}\n");
    let meta = fs::read_to_string(d.path().join(".niutero").join("meta.json")).unwrap();
    assert!(!meta.contains("\"a\""), "meta should no longer mention 'a'");
}

#[test]
fn rm_keeps_verbatim_blocks() {
    let d = new_vault();
    set_bib(&d, "@string{s = {S}}\n\n@misc{gone\n}\n");
    niutero()
        .arg("rm")
        .arg(d.path())
        .arg("gone")
        .assert()
        .success();
    assert_eq!(bib(&d), "@string{s = {S}}\n");
}

#[test]
fn rm_missing_errors() {
    let d = new_vault();
    niutero()
        .arg("rm")
        .arg(d.path())
        .arg("ghost")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no entry with cite key 'ghost'"));
}

// ---------------------------------------------------------------- combined

#[test]
fn add_then_edit_then_rm_is_consistent() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "x", "--field", "title=A"])
        .assert()
        .success();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "y", "--field", "title=B"])
        .assert()
        .success();
    niutero()
        .arg("edit")
        .arg(d.path())
        .arg("x")
        .args(["--field", "year=2024"])
        .assert()
        .success();
    niutero()
        .arg("rm")
        .arg(d.path())
        .arg("y")
        .assert()
        .success();
    assert_eq!(bib(&d), "@misc{x,\n  title = {A},\n  year = {2024}\n}\n");
}

// ------------------------------------------------ validation (review C1)

#[test]
fn add_rejects_illegal_citekey_without_writing() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "x} @evil{y"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("illegal character"));
    // The corrupt entry was never written.
    assert_eq!(bib(&d), "");
}

#[test]
fn add_rejects_unbalanced_value_without_writing() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "ok", "--field", "title=x}"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unbalanced"));
    assert_eq!(bib(&d), "");
}

#[test]
fn edit_rejects_unbalanced_value_preserving_original() {
    let d = new_vault();
    set_bib(&d, "@misc{k,\n  title = {ok}\n}\n");
    niutero()
        .arg("edit")
        .arg(d.path())
        .arg("k")
        .args(["--field", "title=a}b{c"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unbalanced"));
    assert_eq!(bib(&d), "@misc{k,\n  title = {ok}\n}\n");
}

#[test]
fn add_accepts_balanced_tricky_value() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args([
            "--type",
            "misc",
            "--key",
            "k",
            "--field",
            "title=Hello {World} and \"q\" # x",
        ])
        .assert()
        .success();
    assert_eq!(
        bib(&d),
        "@misc{k,\n  title = {Hello {World} and \"q\" # x}\n}\n"
    );
}

// ------------------------------------------------------------- --json output

#[test]
fn mutating_commands_emit_json() {
    let d = new_vault();

    // add --json → {"added": [...]}
    let out = niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "k", "--field", "title=T"])
        .arg("--json")
        .assert()
        .success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("\"added\""), "got: {s}");
    assert!(s.contains("\"k\""), "got: {s}");

    // tag --json → {"tags": ["nlp"]}
    let out = niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .args(["--add", "nlp"])
        .arg("--json")
        .assert()
        .success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("\"tags\"") && s.contains("nlp"), "got: {s}");

    // status --json → {"status":"reading"}
    let out = niutero()
        .arg("status")
        .arg(d.path())
        .arg("k")
        .args(["--set", "reading"])
        .arg("--json")
        .assert()
        .success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        s.contains("\"status\"") && s.contains("reading"),
        "got: {s}"
    );

    // stars --json → {"stars":3}
    let out = niutero()
        .arg("stars")
        .arg(d.path())
        .arg("k")
        .args(["--set", "3"])
        .arg("--json")
        .assert()
        .success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("\"stars\"") && s.contains('3'), "got: {s}");

    // rm --json → {"removed":"k"}
    let out = niutero()
        .arg("rm")
        .arg(d.path())
        .arg("k")
        .arg("--json")
        .assert()
        .success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("\"removed\"") && s.contains("\"k\""), "got: {s}");
}

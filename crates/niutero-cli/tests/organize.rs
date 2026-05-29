//! Black-box tests for the M3 sidecar commands: tag / note / view. These
//! touch only `.niutero/` — `references.bib` must stay untouched.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn niutero() -> Command {
    isolate_registry();
    Command::cargo_bin("niutero").expect("binary built")
}

/// Point the machine-local registry at a per-binary temp file (inherited by the
/// spawned process) so tests never touch — or race on — the real machine one.
fn isolate_registry() {
    use std::sync::OnceLock;
    static REG: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = REG.get_or_init(|| tempfile::tempdir().expect("registry tempdir"));
    std::env::set_var("NIUTERO_REGISTRY", dir.path().join("vaults.toml"));
}

fn vault_with_entry() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    fs::write(
        d.path().join("references.bib"),
        "@misc{k,\n  title = {T}\n}\n",
    )
    .unwrap();
    d
}

fn bib(d: &TempDir) -> String {
    fs::read_to_string(d.path().join("references.bib")).unwrap()
}

fn stdout_of(a: assert_cmd::assert::Assert) -> String {
    String::from_utf8(a.get_output().stdout.clone()).unwrap()
}

// --------------------------------------------------------------------- tag

#[test]
fn tag_add_show_remove_and_bib_untouched() {
    let d = vault_with_entry();
    let before = bib(&d);

    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .args(["--add", "nlp", "--add", "llm"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tags: llm, nlp")); // sorted

    // shown by `show` and by a flagless `tag`
    niutero()
        .arg("show")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("tags: llm, nlp"));
    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("tags: llm, nlp"));

    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .args(["--remove", "llm"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tags: nlp"));

    // Tagging never rewrites the source of truth.
    assert_eq!(bib(&d), before);
}

#[test]
fn tag_missing_entry_errors() {
    let d = vault_with_entry();
    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("ghost")
        .args(["--add", "x"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no entry with cite key 'ghost'"));
}

// -------------------------------------------------------------------- note

#[test]
fn note_set_show_clear() {
    let d = vault_with_entry();
    niutero()
        .arg("note")
        .arg(d.path())
        .arg("k")
        .args(["--set", "seminal paper"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Set note for k"));
    niutero()
        .arg("show")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("note: seminal paper"));
    niutero()
        .arg("note")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("seminal paper"));
    niutero()
        .arg("note")
        .arg(d.path())
        .arg("k")
        .arg("--clear")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared note for k"));
    niutero()
        .arg("note")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("(no note)"));
}

#[test]
fn note_set_and_clear_conflict() {
    let d = vault_with_entry();
    niutero()
        .arg("note")
        .arg(d.path())
        .arg("k")
        .args(["--set", "x", "--clear"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("either --set or --clear"));
}

// -------------------------------------------------------------------- view

#[test]
fn view_add_list_json_remove() {
    let d = vault_with_entry();
    niutero()
        .arg("view")
        .arg(d.path())
        .arg("add")
        .arg("NLP")
        .args(["--query", "tag:nlp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added view 'NLP'"));

    niutero()
        .arg("view")
        .arg(d.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("NLP: tag:nlp"));

    let out = stdout_of(
        niutero()
            .arg("view")
            .arg(d.path())
            .arg("list")
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["name"], "NLP");
    assert_eq!(v[0]["query"], "tag:nlp");

    niutero()
        .arg("view")
        .arg(d.path())
        .arg("add")
        .arg("NLP")
        .args(["--query", "x"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already exists"));

    niutero()
        .arg("view")
        .arg(d.path())
        .arg("rm")
        .arg("NLP")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed view 'NLP'"));

    niutero()
        .arg("view")
        .arg(d.path())
        .arg("rm")
        .arg("NLP")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no saved view named 'NLP'"));
}

#[test]
fn list_uses_a_saved_view() {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    fs::write(
        d.path().join("references.bib"),
        "@misc{a, title={Apple}}\n@misc{b, title={Banana}}\n",
    )
    .unwrap();
    niutero()
        .arg("view")
        .arg(d.path())
        .arg("add")
        .arg("apples")
        .args(["--query", "apple"])
        .assert()
        .success();
    niutero()
        .arg("list")
        .arg(d.path())
        .args(["--view", "apples"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Apple"))
        .stdout(predicate::str::contains("Banana").not())
        .stdout(predicate::str::contains("1 entr(ies)."));
}

//! Black-box tests for the M3 sidecar commands: tag / note / view. These
//! touch only `.niutero/` — `references.bib` must stay untouched.

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

// ----------------------------------------------------- tag vocabulary (tags)

#[test]
fn tags_vocab_list_rename_merge_delete_bib_untouched() {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    fs::write(
        d.path().join("references.bib"),
        "@misc{a,\n  title = {A}\n}\n@misc{b,\n  title = {B}\n}\n",
    )
    .unwrap();
    let before = bib(&d);

    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("a")
        .args(["--add", "topics:interp", "--add", "wf:to-cite"])
        .assert()
        .success();
    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("b")
        .args(["--add", "topics:interp"])
        .assert()
        .success();

    // list (JSON) shows both tags with counts.
    niutero()
        .arg("tags")
        .arg(d.path())
        .arg("list")
        .arg("--json")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"tag\": \"topics:interp\"")
                .and(predicate::str::contains("\"count\": 2"))
                .and(predicate::str::contains("\"tag\": \"wf:to-cite\"")),
        );

    // rename touches both entries.
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["rename", "topics:interp", "topics:mech-interp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 entries"));
    niutero()
        .arg("show")
        .arg(d.path())
        .arg("a")
        .assert()
        .success()
        .stdout(predicate::str::contains("topics:mech-interp"));

    // merge folds wf:to-cite into the renamed tag (only entry a has it).
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["merge", "wf:to-cite", "topics:mech-interp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 entry"));

    // delete removes it from both, leaving no tags.
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["delete", "topics:mech-interp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 entries"));
    niutero()
        .arg("tags")
        .arg(d.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("(no tags)"));

    // The vocabulary lives only in the sidecar — references.bib is untouched.
    assert_eq!(bib(&d), before);
}

#[test]
fn tags_rename_and_delete_json_shapes() {
    // Pins the documented JSON contracts for script/GUI consumers.
    let d = vault_with_entry();
    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .args(["--add", "x"])
        .assert()
        .success();
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["rename", "x", "y", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"from\": \"x\"")
                .and(predicate::str::contains("\"to\": \"y\""))
                .and(predicate::str::contains("\"changed\": 1")),
        );
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["merge", "y", "z", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"into\": \"z\"")
                .and(predicate::str::contains("\"changed\": 1")),
        );
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["delete", "z", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"tag\": \"z\"")
                .and(predicate::str::contains("\"changed\": 1")),
        );
}

#[test]
fn tags_rename_refreshes_keep_updated_exports() {
    // A `tag:`-filtered keep-updated target must re-export after a vocabulary
    // mutation (`tags rename`), exactly like per-entry `tag --add` does.
    let d = vault_with_entry();
    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .args(["--add", "thesis"])
        .assert()
        .success();
    let mirror = d.path().join("mirror.bib");
    niutero()
        .arg("export-target")
        .arg(d.path())
        .arg("add")
        .arg(&mirror)
        .args(["--query", "tag:thesis"])
        .assert()
        .success();
    assert!(fs::read_to_string(&mirror).unwrap().contains("@misc{k"));

    // After the rename nothing carries `thesis`, so the mirror empties.
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["rename", "thesis", "topics:thesis"])
        .assert()
        .success();
    assert!(!fs::read_to_string(&mirror).unwrap().contains("@misc{k"));
}

#[test]
fn tags_rename_empty_target_errors() {
    let d = vault_with_entry();
    niutero()
        .arg("tag")
        .arg(d.path())
        .arg("k")
        .args(["--add", "topics:x"])
        .assert()
        .success();
    niutero()
        .arg("tags")
        .arg(d.path())
        .args(["rename", "topics:x", "   "])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("must not be empty"));
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

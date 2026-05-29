//! Black-box CLI tests: run the real `niutero` binary against temp vaults and
//! assert on stdout / stderr / exit code / files on disk.

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

const TWO: &str = r#"@article{shannon1948, title = {A Mathematical Theory}, author = {Shannon, C. E.}, year = {1948}}
@inproceedings{niu2025, title = {Llama See}, year = {2025}}
"#;

/// Init a vault and overwrite its references.bib with `bib`.
fn vault_with(bib: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(dir.path()).assert().success();
    fs::write(dir.path().join("references.bib"), bib).unwrap();
    dir
}

fn stdout_of(assert: assert_cmd::assert::Assert) -> String {
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

#[test]
fn init_creates_vault() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MyLib");
    niutero()
        .arg("init")
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized vault 'MyLib'"));
    assert!(root.join("references.bib").exists());
    assert!(root.join(".niutero").join("config.toml").exists());
    assert!(root.join(".niutero").join("meta.json").exists());
    assert!(root.join("README.md").exists());
    assert!(root.join(".niutero").join("norm.toml").exists());
}

#[test]
fn cite_prints_latex_and_errors_on_missing() {
    let dir = vault_with(TWO);
    niutero()
        .arg("cite")
        .arg(dir.path())
        .arg("shannon1948")
        .assert()
        .success()
        .stdout(predicate::str::contains("\\cite{shannon1948}"));
    niutero()
        .arg("cite")
        .arg(dir.path())
        .arg("nope")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no entry"));
}

#[test]
fn init_existing_errors() {
    let dir = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(dir.path()).assert().success();
    niutero()
        .arg("init")
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already"));
}

#[test]
fn list_empty_vault() {
    let dir = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(dir.path()).assert().success();
    niutero()
        .arg("list")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("0 entr(ies)."));
}

#[test]
fn list_shows_entries() {
    let dir = vault_with(TWO);
    niutero()
        .arg("list")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("shannon1948"))
        .stdout(predicate::str::contains("niu2025"))
        .stdout(predicate::str::contains("2 entr(ies)."));
}

#[test]
fn list_json_is_valid_and_ordered() {
    let dir = vault_with(TWO);
    let out = stdout_of(
        niutero()
            .arg("list")
            .arg(dir.path())
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(v[0]["citekey"], "shannon1948");
    assert_eq!(v[0]["type"], "article");
    assert_eq!(v[0]["fields"]["year"], "1948");
    assert!(v[0]["tags"].as_array().unwrap().is_empty());
}

#[test]
fn list_query_filters() {
    let dir = vault_with(TWO);
    niutero()
        .arg("list")
        .arg(dir.path())
        .args(["--query", "llama"])
        .assert()
        .success()
        .stdout(predicate::str::contains("niu2025"))
        .stdout(predicate::str::contains("shannon1948").not())
        .stdout(predicate::str::contains("1 entr(ies)."));
}

#[test]
fn show_text_and_json() {
    let dir = vault_with(TWO);
    niutero()
        .arg("show")
        .arg(dir.path())
        .arg("shannon1948")
        .assert()
        .success()
        .stdout(predicate::str::contains("@article{shannon1948}"))
        .stdout(predicate::str::contains("A Mathematical Theory"));

    let out = stdout_of(
        niutero()
            .arg("show")
            .arg(dir.path())
            .arg("shannon1948")
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["citekey"], "shannon1948");
    assert_eq!(v["fields"]["author"], "Shannon, C. E.");
}

#[test]
fn show_missing_errors() {
    let dir = vault_with(TWO);
    niutero()
        .arg("show")
        .arg(dir.path())
        .arg("nope")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no entry with cite key 'nope'"));
}

#[test]
fn show_includes_sidecar_tags_and_note() {
    let dir = vault_with(TWO);
    let meta = r#"{ "shannon1948": { "tags": ["info-theory"], "note": "seminal" } }"#;
    fs::write(dir.path().join(".niutero").join("meta.json"), meta).unwrap();
    niutero()
        .arg("show")
        .arg(dir.path())
        .arg("shannon1948")
        .assert()
        .success()
        .stdout(predicate::str::contains("tags: info-theory"))
        .stdout(predicate::str::contains("note: seminal"));
}

#[test]
fn list_view_and_conflicts() {
    let dir = vault_with(TWO);
    fs::write(
        dir.path().join(".niutero").join("views.toml"),
        "[[views]]\nname = \"NLP\"\nquery = \"llama\"\n",
    )
    .unwrap();
    niutero()
        .arg("list")
        .arg(dir.path())
        .args(["--view", "NLP"])
        .assert()
        .success()
        .stdout(predicate::str::contains("niu2025"))
        .stdout(predicate::str::contains("shannon1948").not());

    niutero()
        .arg("list")
        .arg(dir.path())
        .args(["--query", "x", "--view", "NLP"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("either --query or --view"));

    niutero()
        .arg("list")
        .arg(dir.path())
        .args(["--view", "Nope"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no saved view named 'Nope'"));
}

#[test]
fn list_on_missing_vault_errors() {
    let dir = tempfile::tempdir().unwrap();
    niutero()
        .arg("list")
        .arg(dir.path().join("does-not-exist"))
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("open"));
}

#[test]
fn bad_usage_exits_two() {
    // clap's own parse error (missing required arg) exits 2.
    niutero().arg("show").assert().failure().code(2);
}

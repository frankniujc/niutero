//! Black-box tests for the `config` command (the library's own
//! `.niutero/config.toml`) and the behaviors hanging off it: the on-duplicate
//! import default, the auto-commit hook, and the remote shown by
//! `sync-config`. All offline (git operations are local-only).

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

fn niutero(reg: &Path) -> Command {
    let mut c = Command::cargo_bin("niutero-cli").expect("binary built");
    c.env("NIUTERO_REGISTRY", reg);
    c
}

fn reg_file(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("vaults.toml")
}

fn vault(reg: &Path) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero(reg).arg("init").arg(d.path()).assert().success();
    d
}

#[test]
fn config_get_set_roundtrips_into_the_vaults_own_toml() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault(&reg);

    // Defaults.
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("citekey pattern: (default)")
                .and(predicate::str::contains("on duplicate:     (tool default)"))
                .and(predicate::str::contains("auto-commit:      false")),
        );

    // Set everything; values echo back.
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .args([
            "--name",
            "My Papers",
            "--pattern",
            "{auth}{year}",
            "--enrich-on-import",
            "true",
            "--auto-commit",
            "true",
            "--on-dup",
            "overwrite",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("name:            My Papers")
                .and(predicate::str::contains("citekey pattern: {auth}{year}"))
                .and(predicate::str::contains("on duplicate:     overwrite")),
        );

    // The values live in the VAULT's config.toml (synced with the library),
    // not in the machine registry.
    let toml = fs::read_to_string(d.path().join(".niutero").join("config.toml")).unwrap();
    assert!(toml.contains("My Papers"), "{toml}");
    assert!(toml.contains("enrich_on_import"), "{toml}");
    assert!(toml.contains("on_dup"), "{toml}");

    // JSON shape for machine consumers.
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .arg("--json")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"enrich_on_import\": true")
                .and(predicate::str::contains("\"on_dup\": \"overwrite\"")),
        );

    // Validation: a junk policy is refused.
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .args(["--on-dup", "explode"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown duplicate policy"));
}

#[test]
fn import_uses_the_configured_on_dup_default() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault(&reg);
    fs::write(
        d.path().join("references.bib"),
        "@misc{k,\n  title = {Old}\n}\n",
    )
    .unwrap();
    let incoming = d.path().join("in.bib");
    fs::write(&incoming, "@misc{k,\n  title = {New}\n}\n").unwrap();

    // Tool default (skip): the old entry survives.
    niutero(&reg)
        .arg("import")
        .arg(d.path())
        .arg(&incoming)
        .assert()
        .success()
        .stdout(predicate::str::contains("1 skipped"));

    // Configure overwrite as the library default; no flag needed anymore.
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .args(["--on-dup", "overwrite"])
        .assert()
        .success();
    niutero(&reg)
        .arg("import")
        .arg(d.path())
        .arg(&incoming)
        .assert()
        .success()
        .stdout(predicate::str::contains("1 overwritten"));
    assert!(fs::read_to_string(d.path().join("references.bib"))
        .unwrap()
        .contains("New"));

    // An explicit flag still wins over the configured default.
    niutero(&reg)
        .arg("import")
        .arg(d.path())
        .arg(&incoming)
        .args(["--on-dup", "skip"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 skipped"));
}

#[test]
fn auto_commit_fires_after_mutations_when_enabled() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault(&reg);

    // Make the vault a repo (local-only; the fake remote is never contacted).
    niutero(&reg)
        .arg("connect")
        .arg(d.path())
        .arg("https://example.com/lib.git")
        .assert()
        .success();

    // Pref off: a mutation leaves no auto-commit marker.
    let out = niutero(&reg)
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "a", "--field", "title=A"])
        .assert()
        .success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(!stderr.contains("auto-committed"), "stderr: {stderr}");

    // Pref on: the next mutation commits (covering both itself and backlog).
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .args(["--auto-commit", "true"])
        .assert()
        .success()
        .stderr(predicate::str::contains("auto-committed"));
    niutero(&reg)
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "b", "--field", "title=B"])
        .assert()
        .success()
        .stderr(predicate::str::contains("auto-committed"));

    // Sidecar-only mutations commit too (a note dirties the synced sidecar).
    niutero(&reg)
        .arg("note")
        .arg(d.path())
        .args(["b", "--set", "read me"])
        .assert()
        .success()
        .stderr(predicate::str::contains("auto-committed"));

    // And the work tree ends clean: `history` sees the committed entry.
    niutero(&reg)
        .arg("history")
        .arg(d.path())
        .arg("b")
        .assert()
        .success();
}

#[test]
fn sync_config_shows_the_remote_read_from_the_repo() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault(&reg);

    niutero(&reg)
        .arg("sync-config")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("(none — run `connect`)"));

    niutero(&reg)
        .arg("connect")
        .arg(d.path())
        .arg("git@github.com:frank/lib.git")
        .assert()
        .success();
    niutero(&reg)
        .arg("sync-config")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("git@github.com:frank/lib.git"));
    niutero(&reg)
        .arg("sync-config")
        .arg(d.path())
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"remote\": \"git@github.com:frank/lib.git\"",
        ));
}

#[test]
fn pdf_repo_lands_in_the_vault_config_not_the_registry() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault(&reg);

    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args(["--repo", "frank/papers"])
        .assert()
        .success()
        .stdout(predicate::str::contains("frank/papers"));

    let toml = fs::read_to_string(d.path().join(".niutero").join("config.toml")).unwrap();
    assert!(toml.contains("frank/papers"), "{toml}");
    // `config` surfaces it too (one view of the library's own file).
    niutero(&reg)
        .arg("config")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("pdf repo:        frank/papers"));
    // Shape validation up front.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args(["--repo", "not a repo"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("isn't a valid HF dataset id"));
}

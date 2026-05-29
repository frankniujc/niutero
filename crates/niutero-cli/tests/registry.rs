//! Black-box tests for the machine-local registry commands: recent vaults,
//! keep-updated export targets (#45), and sync-strategy config (#48). Each test
//! points `$NIUTERO_REGISTRY` at its own temp file (passed explicitly per
//! command) so they're fully isolated from each other and from the real machine
//! registry.

use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A `niutero` command whose registry is the given (isolated) file.
fn niutero(reg: &Path) -> Command {
    let mut c = Command::cargo_bin("niutero").expect("binary built");
    c.env("NIUTERO_REGISTRY", reg);
    c
}

fn stdout_of(a: assert_cmd::assert::Assert) -> String {
    String::from_utf8(a.get_output().stdout.clone()).unwrap()
}

/// (vault dir, registry dir, registry file path) — all isolated to this test.
fn setup() -> (TempDir, TempDir, std::path::PathBuf) {
    let vault = tempfile::tempdir().unwrap();
    let regdir = tempfile::tempdir().unwrap();
    let reg = regdir.path().join("vaults.toml");
    niutero(&reg)
        .arg("init")
        .arg(vault.path())
        .assert()
        .success();
    (vault, regdir, reg)
}

fn add(reg: &Path, vault: &Path, key: &str, title: &str) {
    niutero(reg)
        .arg("add")
        .arg(vault)
        .args(["--type", "misc", "--key", key])
        .args(["--field", &format!("title={title}")])
        .assert()
        .success();
}

#[test]
fn recent_lists_opened_vaults_most_recent_first() {
    let regdir = tempfile::tempdir().unwrap();
    let reg = regdir.path().join("vaults.toml");
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    niutero(&reg).arg("init").arg(a.path()).assert().success();
    niutero(&reg).arg("init").arg(b.path()).assert().success();
    // Re-open A so it becomes the most recent.
    niutero(&reg).arg("list").arg(a.path()).assert().success();

    let out = stdout_of(niutero(&reg).arg("recent").assert().success());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "expected two recent vaults, got: {out}");
    // A is most recent (re-opened last), so it heads the list.
    assert!(lines[0].contains(a.path().file_name().unwrap().to_str().unwrap()));

    // JSON form carries the paths too.
    let json = stdout_of(niutero(&reg).arg("recent").arg("--json").assert().success());
    assert!(json.contains("\"path\""), "json: {json}");
}

#[test]
fn forget_drops_a_vault_from_recent() {
    let (vault, _regdir, reg) = setup();
    niutero(&reg)
        .arg("forget")
        .arg(vault.path())
        .assert()
        .success();
    let out = stdout_of(niutero(&reg).arg("recent").assert().success());
    assert!(out.contains("(no recent vaults)"), "got: {out}");
}

#[test]
fn export_target_keeps_an_external_bib_updated() {
    let (vault, _regdir, reg) = setup();
    let mirror_dir = tempfile::tempdir().unwrap();
    let mirror = mirror_dir.path().join("mirror.bib");

    add(&reg, vault.path(), "a", "First");
    // Registering exports immediately.
    niutero(&reg)
        .arg("export-target")
        .arg(vault.path())
        .arg("add")
        .arg(&mirror)
        .assert()
        .success();
    assert!(fs::read_to_string(&mirror).unwrap().contains("@misc{a"));

    // A later add auto-refreshes the mirror (no extra command needed).
    add(&reg, vault.path(), "b", "Second");
    let mirrored = fs::read_to_string(&mirror).unwrap();
    assert!(
        mirrored.contains("@misc{a") && mirrored.contains("@misc{b"),
        "mirror not refreshed: {mirrored}"
    );

    // List shows it; Rm stops tracking.
    let listed = stdout_of(
        niutero(&reg)
            .arg("export-target")
            .arg(vault.path())
            .arg("list")
            .assert()
            .success(),
    );
    assert!(listed.contains("mirror.bib"), "got: {listed}");

    niutero(&reg)
        .arg("export-target")
        .arg(vault.path())
        .arg("rm")
        .arg(&mirror)
        .assert()
        .success();
    // After removal a further change no longer touches the mirror.
    add(&reg, vault.path(), "c", "Third");
    assert!(!fs::read_to_string(&mirror).unwrap().contains("@misc{c"));
}

#[test]
fn export_target_refuses_the_vaults_own_bib() {
    let (vault, _regdir, reg) = setup();
    niutero(&reg)
        .arg("export-target")
        .arg(vault.path())
        .arg("add")
        .arg(vault.path().join("references.bib"))
        .assert()
        .failure();
}

#[test]
fn sync_config_persists_and_shows() {
    let (vault, _regdir, reg) = setup();
    // Default is two-way.
    let shown = stdout_of(
        niutero(&reg)
            .arg("sync-config")
            .arg(vault.path())
            .assert()
            .success(),
    );
    assert!(shown.contains("pull=true, push=true"), "got: {shown}");

    // Set push off; it persists across invocations.
    niutero(&reg)
        .arg("sync-config")
        .arg(vault.path())
        .args(["--push", "false"])
        .assert()
        .success();
    let after = stdout_of(
        niutero(&reg)
            .arg("sync-config")
            .arg(vault.path())
            .assert()
            .success(),
    );
    assert!(after.contains("pull=true, push=false"), "got: {after}");
}

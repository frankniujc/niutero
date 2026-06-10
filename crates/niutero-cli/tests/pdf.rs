//! Black-box tests for the PDF surface: `pdf-config`, the offline gates of
//! `pdf --push/--pull`, the real `has_pdf` flag, and the import auto-fetch
//! hook. Live HF calls (create/push/pull against huggingface.co) are not
//! exercised here — only their offline refusals.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

/// A niutero command with the machine registry isolated per test, passed on
/// the spawned process's environment (never the harness's process-global env).
fn niutero(reg: &Path) -> Command {
    let mut c = Command::cargo_bin("niutero-cli").expect("binary built");
    c.env("NIUTERO_REGISTRY", reg);
    c
}

fn reg_file(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("vaults.toml")
}

fn vault_with_entry(reg: &Path) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero(reg).arg("init").arg(d.path()).assert().success();
    fs::write(
        d.path().join("references.bib"),
        "@misc{k,\n  title = {T}\n}\n",
    )
    .unwrap();
    d
}

#[test]
fn pdf_config_roundtrips_and_never_echoes_the_token() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault_with_entry(&reg);

    // Defaults: unset repo, auto-fetch off, no token.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("repo:       (unset)")
                .and(predicate::str::contains("auto-fetch: false"))
                .and(predicate::str::contains("hf token:   (unset)")),
        );

    // Set everything; the token arrives on stdin and never appears in output.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args([
            "--repo",
            "frank/papers",
            "--auto-fetch",
            "true",
            "--token-stdin",
        ])
        .write_stdin("hf_supersecret_123\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("hf_supersecret_123")
                .not()
                .and(predicate::str::contains("frank/papers"))
                .and(predicate::str::contains("set (machine-local)")),
        );

    // JSON round-trip exposes only token_set, never the token.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .arg("--json")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"repo\": \"frank/papers\"")
                .and(predicate::str::contains("\"auto_fetch\": true"))
                .and(predicate::str::contains("\"token_set\": true"))
                .and(predicate::str::contains("hf_supersecret_123").not()),
        );

    // Clearing the token works (empty string via --token).
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args(["--token", ""])
        .assert()
        .success()
        .stdout(predicate::str::contains("hf token:   (unset)"));
}

#[test]
fn pdf_push_pull_gate_offline_with_actionable_errors() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault_with_entry(&reg);

    // No token → refuse before any network call.
    niutero(&reg)
        .arg("pdf")
        .arg(d.path())
        .args(["k", "--push"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no HuggingFace token"));

    // Token but no repo → the repo hint.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args(["--token", "hf_x"])
        .assert()
        .success();
    niutero(&reg)
        .arg("pdf")
        .arg(d.path())
        .args(["k", "--pull"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no HF dataset repo"));

    // Token + repo but nothing attached → push explains, still offline.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args(["--repo", "u/r"])
        .assert()
        .success();
    niutero(&reg)
        .arg("pdf")
        .arg(d.path())
        .args(["k", "--push"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no local PDF"));

    // Flags are mutually exclusive.
    niutero(&reg)
        .arg("pdf")
        .arg(d.path())
        .args(["k", "--push", "--pull"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("only one of"));
}

#[test]
fn show_reports_real_has_pdf_after_attach() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault_with_entry(&reg);

    // A url alone must not read as an attached PDF.
    niutero(&reg)
        .arg("edit")
        .arg(d.path())
        .args(["k", "--field", "url=https://x.org/a.pdf"])
        .assert()
        .success();
    niutero(&reg)
        .arg("show")
        .arg(d.path())
        .args(["k", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"has_pdf\": false"));

    // After a real attach, the flag flips.
    let src = d.path().join("paper.pdf");
    fs::write(&src, b"%PDF-1.4 fake").unwrap();
    niutero(&reg)
        .arg("pdf")
        .arg(d.path())
        .arg("k")
        .arg("--attach")
        .arg(&src)
        .assert()
        .success();
    niutero(&reg)
        .arg("show")
        .arg(d.path())
        .args(["k", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"has_pdf\": true"));
}

#[test]
fn import_auto_fetch_is_opt_in() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = vault_with_entry(&reg);
    let incoming = d.path().join("in.bib");
    // `.invalid` is reserved-unresolvable (RFC 2606): with the pref ON the
    // attempt fails fast at DNS — proving the hook ran without needing a
    // reachable network.
    fs::write(
        &incoming,
        "@misc{p1,\n  title = {P},\n  url = {https://host.invalid/a.pdf}\n}\n",
    )
    .unwrap();

    // Default (pref off): no auto-fetch marker at all — the import is offline.
    let out = niutero(&reg)
        .arg("import")
        .arg(d.path())
        .arg(&incoming)
        .assert()
        .success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(!stderr.contains("auto-fetched"), "stderr: {stderr}");

    // Pref on: the hook attempts (and fast-fails) the unresolvable host —
    // reported on stderr, never failing the import itself.
    niutero(&reg)
        .arg("pdf-config")
        .arg(d.path())
        .args(["--auto-fetch", "true"])
        .assert()
        .success();
    fs::write(
        &incoming,
        "@misc{p2,\n  title = {P2},\n  url = {https://host.invalid/b.pdf}\n}\n",
    )
    .unwrap();
    niutero(&reg)
        .arg("import")
        .arg(d.path())
        .arg(&incoming)
        .assert()
        .success()
        .stderr(predicate::str::contains("auto-fetched 0/1"));
}

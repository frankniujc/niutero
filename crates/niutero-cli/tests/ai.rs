//! Black-box tests for the `ai` command group. Only the offline-deterministic
//! paths are covered: the config round-trip, the disabled/no-key gates, key
//! masking, and `ai organize --plan/--apply` (fully offline by design). The
//! live network calls (`ai test` with a real key, `ai ask` answers, model-built
//! plans) are exercised manually against the real API.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// A niutero command with the machine registry isolated **per test**, passed
/// on the spawned process's environment (never via the test harness's own
/// process-global env, which parallel test threads would race on).
fn niutero(reg: &Path) -> Command {
    let mut c = Command::cargo_bin("niutero-cli").expect("binary built");
    c.env("NIUTERO_REGISTRY", reg);
    // No ambient key may leak into the disabled/no-key assertions.
    c.env_remove("ANTHROPIC_API_KEY");
    c
}

fn reg_file(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("vaults.toml")
}

#[test]
fn ai_config_gate_then_roundtrip() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);

    // Default: disabled — `test` refuses before any network call.
    niutero(&reg)
        .args(["ai", "test"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("off"));

    // Configure it.
    niutero(&reg)
        .args([
            "ai",
            "config",
            "--enable",
            "true",
            "--key",
            "sk-test-123456",
            "--model",
            "claude-x",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("enabled:  true"));

    // Show round-trips (JSON): enabled, model set, key present (never shown).
    niutero(&reg)
        .args(["ai", "config", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"enabled\": true")
                .and(predicate::str::contains("\"model\": \"claude-x\""))
                .and(predicate::str::contains("\"api_key_set\": true"))
                .and(predicate::str::contains("sk-test-123456").not()),
        );
}

#[test]
fn ai_config_key_stdin_and_text_masking() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);

    // The key arrives on stdin — never argv — and text output only hints it.
    niutero(&reg)
        .args(["ai", "config", "--key-stdin"])
        .write_stdin("sk-ant-supersecret-123\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("sk-ant-supersecret-123")
                .not()
                .and(predicate::str::contains("sk-ant…")),
        );

    // A short key never echoes at all (a 6-char "mask" would be the full key).
    niutero(&reg)
        .args(["ai", "config", "--key", "abc"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("abc")
                .not()
                .and(predicate::str::contains("(set — 3 chars)")),
        );
}

#[test]
fn ai_ask_and_organize_gate_offline_when_disabled() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = tempfile::tempdir().unwrap();
    niutero(&reg).arg("init").arg(d.path()).assert().success();

    // `ask` reports the master switch before any network call (and smoke-tests
    // the vault + positional-question arg shape).
    niutero(&reg)
        .args(["ai", "ask"])
        .arg(d.path())
        .arg("what's in here?")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("off"));

    // `organize` (the model path) gates the same way — even on an empty vault,
    // pinning that the gate runs before the empty-vocabulary early return.
    niutero(&reg)
        .args(["ai", "organize"])
        .arg(d.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("off"));
}

#[test]
fn ai_organize_plan_apply_offline_bib_untouched() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = tempfile::tempdir().unwrap();
    niutero(&reg).arg("init").arg(d.path()).assert().success();
    std::fs::write(
        d.path().join("references.bib"),
        "@misc{a,\n  title = {A}\n}\n@misc{b,\n  title = {B}\n}\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(d.path().join("references.bib")).unwrap();
    niutero(&reg)
        .arg("tag")
        .arg(d.path())
        .arg("a")
        .args(["--add", "topics:ml"])
        .assert()
        .success();
    niutero(&reg)
        .arg("tag")
        .arg(d.path())
        .arg("b")
        .args(["--add", "topics:m-l"])
        .assert()
        .success();

    // A handcrafted plan: one real merge, one stale merge, one advisory tag.
    let plan = d.path().join("plan.json");
    std::fs::write(
        &plan,
        r#"{"merges":[{"from":"topics:m-l","into":"topics:ml"},{"from":"ghost","into":"x"}],"new_tags":[{"name":"topics:nlp","reason":"recurring"}]}"#,
    )
    .unwrap();

    // Plan-only: prints the plan, applies nothing, needs no AI config at all.
    niutero(&reg)
        .args(["ai", "organize"])
        .arg(d.path())
        .arg("--plan")
        .arg(&plan)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("merge 'topics:m-l' → 'topics:ml'")
                .and(predicate::str::contains("advisory")),
        );
    niutero(&reg)
        .arg("tags")
        .arg(d.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("topics:m-l"));

    // Apply: the real merge lands, the stale one is skipped (not a failure),
    // the suggested new tag is NOT created, and references.bib is untouched.
    let out = niutero(&reg)
        .args(["ai", "organize"])
        .arg(d.path())
        .arg("--plan")
        .arg(&plan)
        .args(["--apply", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["changed_total"], 1);
    assert_eq!(v["applied"][0]["changed"], 1);
    assert_eq!(v["applied"][1]["changed"], 0);

    niutero(&reg)
        .arg("tags")
        .arg(d.path())
        .arg("list")
        .arg("--json")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"tag\": \"topics:ml\"")
                .and(predicate::str::contains("\"count\": 2"))
                .and(predicate::str::contains("topics:m-l").not())
                .and(predicate::str::contains("topics:nlp").not()),
        );
    assert_eq!(
        std::fs::read_to_string(d.path().join("references.bib")).unwrap(),
        before
    );
}

#[test]
fn ai_organize_json_round_trips_as_plan_input() {
    let t = tempfile::tempdir().unwrap();
    let reg = reg_file(&t);
    let d = tempfile::tempdir().unwrap();
    niutero(&reg).arg("init").arg(d.path()).assert().success();
    let plan = d.path().join("plan.json");
    std::fs::write(
        &plan,
        r#"{"merges":[{"from":"a","into":"b","reason":"r"}],"new_tags":[]}"#,
    )
    .unwrap();
    let out = niutero(&reg)
        .args(["ai", "organize"])
        .arg(d.path())
        .arg("--plan")
        .arg(&plan)
        .arg("--json")
        .assert()
        .success();
    let json1 = String::from_utf8(out.get_output().stdout.clone()).unwrap();

    // Feeding the --json output back as --plan yields the identical plan.
    let plan2 = d.path().join("plan2.json");
    std::fs::write(&plan2, &json1).unwrap();
    let out2 = niutero(&reg)
        .args(["ai", "organize"])
        .arg(d.path())
        .arg("--plan")
        .arg(&plan2)
        .arg("--json")
        .assert()
        .success();
    let json2 = String::from_utf8(out2.get_output().stdout.clone()).unwrap();
    assert_eq!(json1, json2);
}

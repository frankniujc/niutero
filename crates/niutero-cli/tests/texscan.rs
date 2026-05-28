//! Black-box tests for `tex-scan` (M5 LaTeX glue), including the exit-2
//! CI-gate behavior on undefined references.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn niutero() -> Command {
    Command::cargo_bin("niutero").expect("binary built")
}

fn vault_with(contents: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    fs::write(d.path().join("references.bib"), contents).unwrap();
    d
}

fn write_tex(d: &TempDir, name: &str, contents: &str) -> PathBuf {
    let p = d.path().join(name);
    fs::write(&p, contents).unwrap();
    p
}

#[test]
fn missing_reference_exits_two() {
    let d = vault_with("@misc{a, title={A}}\n@misc{b, title={B}}\n");
    let tex = write_tex(&d, "paper.tex", r"\cite{a,missing1}");
    niutero()
        .arg("tex-scan")
        .arg(d.path())
        .arg(&tex)
        .assert()
        .code(2)
        .stdout(predicate::str::contains("used 1, missing 1, unused 1"))
        .stdout(predicate::str::contains("missing1"));
}

#[test]
fn all_cited_present_exits_zero() {
    let d = vault_with("@misc{a, title={A}}\n");
    let tex = write_tex(&d, "p.tex", r"\cite{a}");
    niutero()
        .arg("tex-scan")
        .arg(d.path())
        .arg(&tex)
        .assert()
        .success()
        .stdout(predicate::str::contains("used 1, missing 0, unused 0"));
}

#[test]
fn nocite_star_has_no_unused() {
    let d = vault_with("@misc{a}\n@misc{b}\n");
    let tex = write_tex(&d, "p.tex", r"\nocite{*}");
    niutero()
        .arg("tex-scan")
        .arg(d.path())
        .arg(&tex)
        .assert()
        .success()
        .stdout(predicate::str::contains("unused 0"))
        .stdout(predicate::str::contains("nocite"));
}

#[test]
fn out_writes_pruned_bib() {
    let d = vault_with("@article{a, title={Apple}}\n@misc{b, title={Banana}}\n");
    let tex = write_tex(&d, "p.tex", r"\cite{a}");
    let out = d.path().join("pruned.bib");
    niutero()
        .arg("tex-scan")
        .arg(d.path())
        .arg(&tex)
        .arg("--out")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Wrote 1 cited"));
    let w = fs::read_to_string(&out).unwrap();
    assert!(w.contains("@article{a,"));
    assert!(!w.contains("@misc{b,"));
}

#[test]
fn json_shape() {
    let d = vault_with("@misc{a}\n@misc{b}\n");
    let tex = write_tex(&d, "p.tex", r"\cite{a,z}");
    let assert = niutero()
        .arg("tex-scan")
        .arg(d.path())
        .arg(&tex)
        .arg("--json")
        .assert()
        .code(2); // undefined ref `z`
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["used"], serde_json::json!(["a"]));
    assert_eq!(v["missing"], serde_json::json!(["z"]));
    assert_eq!(v["unused"], serde_json::json!(["b"]));
}

#[test]
fn scans_multiple_files() {
    let d = vault_with("@misc{a}\n@misc{b}\n");
    let t1 = write_tex(&d, "intro.tex", r"\cite{a}");
    let t2 = write_tex(&d, "body.tex", r"\citep{b}");
    niutero()
        .arg("tex-scan")
        .arg(d.path())
        .arg(&t1)
        .arg(&t2)
        .assert()
        .success()
        .stdout(predicate::str::contains("used 2, missing 0, unused 0"));
}

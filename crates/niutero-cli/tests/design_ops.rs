//! Black-box tests for the design-driven operations: citation-key pattern +
//! re-key, reading status / stars, and the offline analyze report.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn niutero() -> Command {
    Command::cargo_bin("niutero").expect("binary built")
}

fn new_vault() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    d
}

fn bib(d: &TempDir) -> String {
    fs::read_to_string(d.path().join("references.bib")).unwrap()
}

fn stdout_of(a: assert_cmd::assert::Assert) -> String {
    String::from_utf8(a.get_output().stdout.clone()).unwrap()
}

// ------------------------------------------------------------ auto-key + rekey

#[test]
fn add_without_key_generates_one_from_the_pattern() {
    let d = new_vault();
    let out = stdout_of(
        niutero()
            .arg("add")
            .arg(d.path())
            .args(["--type", "article"])
            .args(["--field", "author=Vaswani, Ashish"])
            .args(["--field", "year=2017"])
            .args(["--field", "title=Attention Is All You Need"])
            .assert()
            .success(),
    );
    assert!(out.contains("vaswani2017attentionIsAll"), "got: {out}");
    assert!(bib(&d).contains("@article{vaswani2017attentionIsAll,"));
}

#[test]
fn add_key_without_type_errors() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--key", "k"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--key requires --type"));
}

#[test]
fn rekey_previews_then_writes() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        "@article{OldKey,\n  author = {Arad, Dana},\n  year = {2025},\n  title = {SAEs Are Good}\n}\n",
    )
    .unwrap();

    // preview: shows the rename, writes nothing
    let preview = stdout_of(niutero().arg("rekey").arg(d.path()).assert().success());
    assert!(
        preview.contains("OldKey -> arad2025saesAreGood"),
        "got: {preview}"
    );
    assert!(
        bib(&d).contains("@article{OldKey,"),
        "preview must not write"
    );

    // write: applies it
    niutero()
        .arg("rekey")
        .arg(d.path())
        .arg("--write")
        .assert()
        .success();
    let after = bib(&d);
    assert!(after.contains("@article{arad2025saesAreGood,"));
    assert!(!after.contains("OldKey"));
}

#[test]
fn rekey_with_pattern_override_as_json() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        "@misc{x,\n  author = {Bricken, Trenton},\n  year = {2023},\n  title = {Toward Mono}\n}\n",
    )
    .unwrap();
    let out = stdout_of(
        niutero()
            .arg("rekey")
            .arg(d.path())
            .args(["--pattern", "{auth}{year}"])
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["new_key"], "bricken2023");
}

// ----------------------------------------------------------------- status/stars

#[test]
fn status_and_stars_set_and_show_without_touching_bib() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        "@misc{k,\n  title = {T}\n}\n",
    )
    .unwrap();
    let before = bib(&d);

    niutero()
        .arg("status")
        .arg(d.path())
        .arg("k")
        .args(["--set", "reading"])
        .assert()
        .success()
        .stdout(predicate::str::contains("reading"));
    niutero()
        .arg("stars")
        .arg(d.path())
        .arg("k")
        .args(["--set", "4"])
        .assert()
        .success();

    // show reflects both
    niutero()
        .arg("status")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("reading"));
    niutero()
        .arg("stars")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("4"));

    // the .bib is never touched by sidecar writes
    assert_eq!(bib(&d), before);

    // out-of-range rating is rejected
    niutero()
        .arg("stars")
        .arg(d.path())
        .arg("k")
        .args(["--set", "9"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("between 0 and 5"));
}

#[test]
fn list_filters_by_status() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        "@misc{a,\n  title = {A}\n}\n\n@misc{b,\n  title = {B}\n}\n",
    )
    .unwrap();
    niutero()
        .arg("status")
        .arg(d.path())
        .arg("a")
        .args(["--set", "done"])
        .assert()
        .success();
    let out = stdout_of(
        niutero()
            .arg("list")
            .arg(d.path())
            .args(["--query", "status:done"])
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["citekey"], "a");
    assert_eq!(v[0]["status"], "done");
}

// --------------------------------------------------------------------- analyze

#[test]
fn analyze_reports_offline_health() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        concat!(
            "@article{a,\n  title = {A NICE PAPER},\n  journal = {ICLR}\n}\n\n",
            "@article{b,\n  title = {Another Paper},\n  journal = {iclr},\n  year = {2024},\n  url = {http://x}\n}\n",
        ),
    )
    .unwrap();

    // text summary
    niutero()
        .arg("analyze")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("2 entr(ies) scanned"))
        .stdout(predicate::str::contains("Missing year"));

    // json carries the per-check key lists
    let out = stdout_of(
        niutero()
            .arg("analyze")
            .arg(d.path())
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["total"], 2);
    let venues = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "venues")
        .unwrap();
    // ICLR / iclr are the same venue spelled two ways
    assert_eq!(venues["keys"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------- normalize --json

#[test]
fn normalize_json_emits_field_diffs() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        "@article{k,\n  title = {A  B},\n  abstract = {x}\n}\n",
    )
    .unwrap();
    let out = stdout_of(
        niutero()
            .arg("normalize")
            .arg(d.path())
            .arg("--json")
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let diffs = &v[0]["diffs"];
    // abstract dropped (off the keep-field whitelist)
    assert!(diffs
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["field"] == "abstract" && d["to"].is_null()));
}

#[test]
fn dedupe_lists_then_merges_clusters() {
    let d = new_vault();
    fs::write(
        d.path().join("references.bib"),
        concat!(
            "@article{a,\n  author = {Vaswani, A},\n  year = {2017},\n  title = {Attention},\n  x = {1}\n}\n\n",
            "@article{b,\n  author = {Vaswani, A},\n  year = {2017},\n  title = {Attention},\n  y = {2}\n}\n",
        ),
    )
    .unwrap();

    // preview lists the cluster, writes nothing
    niutero()
        .arg("dedupe")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate cluster"));
    assert!(bib(&d).contains("@article{b,"), "preview must not merge");

    // --merge folds b into a
    niutero()
        .arg("dedupe")
        .arg(d.path())
        .arg("--merge")
        .assert()
        .success()
        .stdout(predicate::str::contains("Merged 1 cluster"));
    let after = bib(&d);
    assert!(after.contains("@article{a,") && !after.contains("@article{b,"));
    assert!(after.contains("x = {1}") && after.contains("y = {2}"));
}

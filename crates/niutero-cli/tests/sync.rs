//! Black-box tests for `connect` / `sync` (M5 git sync). The happy-path test
//! uses a local bare repo as the remote and is skipped if `git` is absent.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use std::process::Command as Proc;
use tempfile::TempDir;

fn niutero() -> Command {
    Command::cargo_bin("niutero").expect("binary built")
}

fn git(dir: &Path, args: &[&str]) {
    Proc::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
}

fn git_available() -> bool {
    Proc::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn new_vault() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    niutero().arg("init").arg(d.path()).assert().success();
    d
}

#[test]
fn sync_without_connect_errors() {
    let d = new_vault();
    niutero()
        .arg("sync")
        .arg(d.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not a git repository"));
}

#[test]
fn connect_then_sync_pushes_to_remote() {
    if !git_available() {
        return;
    }
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);
    let bare = remote.path().to_str().unwrap();

    let d = new_vault();
    niutero()
        .arg("connect")
        .arg(d.path())
        .arg(bare)
        .assert()
        .success()
        .stdout(predicate::str::contains("Connected"));

    // connect git-inited the vault; give it a committer identity for the test
    git(d.path(), &["config", "user.email", "t@e.com"]);
    git(d.path(), &["config", "user.name", "T"]);
    git(d.path(), &["config", "commit.gpgsign", "false"]);

    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "k", "--field", "title=Hi"])
        .assert()
        .success();
    niutero()
        .arg("sync")
        .arg(d.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Synced"));

    // A fresh clone of the remote contains the pushed entry.
    let dst = tempfile::tempdir().unwrap();
    let clone = dst.path().join("clone");
    Proc::new("git")
        .args(["clone", bare, clone.to_str().unwrap()])
        .output()
        .unwrap();
    let bib = std::fs::read_to_string(clone.join("references.bib")).unwrap();
    assert!(bib.contains("@misc{k,"));
}

#[test]
fn history_lists_commits_for_an_entry() {
    if !git_available() {
        return;
    }
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);
    let bare = remote.path().to_str().unwrap();

    let d = new_vault();
    niutero()
        .arg("connect")
        .arg(d.path())
        .arg(bare)
        .assert()
        .success();
    git(d.path(), &["config", "user.email", "t@e.com"]);
    git(d.path(), &["config", "user.name", "T"]);
    git(d.path(), &["config", "commit.gpgsign", "false"]);

    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "k", "--field", "title=One"])
        .assert()
        .success();
    niutero().arg("sync").arg(d.path()).assert().success();
    niutero()
        .arg("edit")
        .arg(d.path())
        .arg("k")
        .args(["--field", "title=Two"])
        .assert()
        .success();
    niutero().arg("sync").arg(d.path()).assert().success();

    // Text: both commits, newest first.
    niutero()
        .arg("history")
        .arg(d.path())
        .arg("k")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 changed"))
        .stdout(predicate::str::contains("initial import"));

    // JSON: an array of two commit objects with the stable field shape.
    let out = niutero()
        .arg("history")
        .arg(d.path())
        .arg("k")
        .arg("--json")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let commits: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(commits.as_array().unwrap().len(), 2);
    assert!(commits[0]["hash"].as_str().unwrap().len() >= 7);
    assert!(commits[0]["subject"]
        .as_str()
        .unwrap()
        .contains("1 changed"));
}

#[test]
fn history_without_a_repo_errors() {
    let d = new_vault();
    niutero()
        .arg("add")
        .arg(d.path())
        .args(["--type", "misc", "--key", "k"])
        .assert()
        .success();
    niutero()
        .arg("history")
        .arg(d.path())
        .arg("k")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not a git repository"));
}

#[test]
fn sync_auto_merges_disjoint_field_edits() {
    if !git_available() {
        return;
    }
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);
    let bare = remote.path().to_str().unwrap();

    // A: connect, add one entry, push.
    let a = new_vault();
    let id = |p: &Path| {
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "T"]);
        git(p, &["config", "commit.gpgsign", "false"]);
    };
    niutero()
        .arg("connect")
        .arg(a.path())
        .arg(bare)
        .assert()
        .success();
    id(a.path());
    niutero()
        .arg("add")
        .arg(a.path())
        .args(["--type", "misc", "--key", "k", "--field", "a=1"])
        .assert()
        .success();
    niutero().arg("sync").arg(a.path()).assert().success();

    // B: clone it.
    let dst = tempfile::tempdir().unwrap();
    let b = dst.path().join("b");
    Proc::new("git")
        .args(["clone", bare, b.to_str().unwrap()])
        .output()
        .unwrap();
    id(&b);

    // A adds field `b`; B adds field `c` to the same entry. git's line merge
    // conflicts, but `sync` auto-resolves it with the structured entry merge.
    niutero()
        .arg("edit")
        .arg(a.path())
        .arg("k")
        .args(["--field", "b=2"])
        .assert()
        .success();
    niutero().arg("sync").arg(a.path()).assert().success();
    niutero()
        .arg("edit")
        .arg(&b)
        .arg("k")
        .args(["--field", "c=3"])
        .assert()
        .success();
    niutero()
        .arg("sync")
        .arg(&b)
        .assert()
        .success()
        .stdout(predicate::str::contains("auto-merged"));

    // B's entry now has all three fields.
    niutero()
        .arg("show")
        .arg(&b)
        .arg("k")
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"a\""))
        .stdout(predicate::str::contains("\"b\""))
        .stdout(predicate::str::contains("\"c\""));
}

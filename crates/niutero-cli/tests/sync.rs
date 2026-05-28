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

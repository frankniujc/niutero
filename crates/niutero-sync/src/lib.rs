//! niutero-sync — git synchronization by shelling out to the system `git`.
//!
//! We never link libgit2 and never touch credentials: `git` uses the user's
//! configured credential helper / SSH agent. These are thin primitives; the
//! orchestration (commit → pull → push, conflict handling) lives in the engine.

use std::path::Path;
use std::process::{Command, Output};

/// Result of a pull: a clean update or an aborted merge conflict.
#[derive(Debug, PartialEq, Eq)]
pub enum PullOutcome {
    Ok,
    Conflict,
}

/// Is a usable `git` on PATH?
pub fn git_available() -> bool {
    run(Path::new("."), &["--version"]).is_ok_and(|o| o.status.success())
}

/// Is `dir` inside a git work tree?
pub fn is_repo(dir: &Path) -> bool {
    run(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o.status.success())
}

/// `git init` in `dir`.
pub fn init(dir: &Path) -> Result<(), String> {
    ok(dir, &["init"]).map(|_| ())
}

/// Point remote `name` at `url` (adding it, or updating an existing one).
pub fn set_remote(dir: &Path, name: &str, url: &str) -> Result<(), String> {
    if ok(dir, &["remote", "add", name, url]).is_err() {
        ok(dir, &["remote", "set-url", name, url])?;
    }
    Ok(())
}

/// The URL of remote `name`, if configured.
pub fn remote_url(dir: &Path, name: &str) -> Option<String> {
    ok(dir, &["remote", "get-url", name])
        .ok()
        .map(|s| s.trim().to_string())
}

/// Stage everything and commit. Returns `false` if there was nothing to commit.
pub fn commit_all(dir: &Path, message: &str) -> Result<bool, String> {
    ok(dir, &["add", "-A"])?;
    if ok(dir, &["status", "--porcelain"])?.trim().is_empty() {
        return Ok(false);
    }
    ok(dir, &["commit", "-m", message])?;
    Ok(true)
}

/// Does the current branch have an upstream set?
pub fn has_upstream(dir: &Path) -> bool {
    ok(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .is_ok()
}

/// `git pull` (merge). On a conflict, aborts the merge to leave a clean tree
/// and reports [`PullOutcome::Conflict`].
pub fn pull(dir: &Path) -> Result<PullOutcome, String> {
    let out = run(dir, &["pull", "--no-rebase", "--no-edit"])?;
    if out.status.success() {
        return Ok(PullOutcome::Ok);
    }
    let unmerged = ok(dir, &["ls-files", "--unmerged"]).unwrap_or_default();
    if unmerged.trim().is_empty() {
        Err(format!("git pull: {}", stderr(&out)))
    } else {
        let _ = run(dir, &["merge", "--abort"]);
        Ok(PullOutcome::Conflict)
    }
}

/// Push the current branch to `origin`, setting upstream.
pub fn push(dir: &Path) -> Result<(), String> {
    ok(dir, &["push", "-u", "origin", "HEAD"]).map(|_| ())
}

/// Set a *local* (repo-scoped) git config value. Never touches global config.
pub fn set_config(dir: &Path, key: &str, value: &str) -> Result<(), String> {
    ok(dir, &["config", "--local", key, value]).map(|_| ())
}

/// Contents of `path` as of the current `HEAD` commit, or `None` if there is no
/// `HEAD` (no commits yet) or the file isn't tracked there. Used to diff the
/// working tree against the last commit for stats-aware commit messages.
pub fn file_at_head(dir: &Path, path: &str) -> Option<String> {
    ok(dir, &["show", &format!("HEAD:{path}")]).ok()
}

/// One commit in a line range's history, as reported by [`log_lines`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full commit hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Author date, strict ISO-8601 (e.g. `2026-05-29T15:38:13+02:00`).
    pub date: String,
    /// Commit subject — the first line of the message.
    pub subject: String,
}

/// History of lines `start..=end` (1-based, inclusive) of `file`, newest commit
/// first — a thin wrapper over `git log -L<start>,<end>:<file>`. `git`
/// interprets the line range against the committed `HEAD`, so callers must
/// derive it from `HEAD`'s content (see [`file_at_head`]), not the working tree.
pub fn log_lines(dir: &Path, file: &str, start: usize, end: usize) -> Result<Vec<Commit>, String> {
    // `--no-patch` drops the per-line diff `-L` would otherwise force, leaving
    // one record per commit; `\x1f` (unit separator) can't appear in a one-line
    // subject, so it's a safe field delimiter.
    let range = format!("-L{start},{end}:{file}");
    let out = ok(
        dir,
        &[
            "log",
            &range,
            "--no-patch",
            "--format=%H%x1f%an%x1f%aI%x1f%s",
        ],
    )?;
    Ok(out.lines().filter_map(parse_commit_line).collect())
}

/// Parse one `%H \x1f %an \x1f %aI \x1f %s` record. Returns `None` for a blank or
/// truncated line so a stray line can never abort the whole history.
fn parse_commit_line(line: &str) -> Option<Commit> {
    let mut f = line.split('\u{1f}');
    let hash = f.next()?.trim().to_string();
    if hash.is_empty() {
        return None;
    }
    Some(Commit {
        hash,
        author: f.next().unwrap_or("").to_string(),
        date: f.next().unwrap_or("").to_string(),
        subject: f.next().unwrap_or("").to_string(),
    })
}

// ----------------------------------------------------------------- helpers

fn run(dir: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "git not found on PATH (install git to use sync)".to_string()
            }
            _ => format!("failed to run git: {e}"),
        })
}

/// Run git and return stdout on success, or a formatted error on failure.
fn ok(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = run(dir, args)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("git {}: {}", args.join(" "), stderr(&out)))
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Init a repo with a committer identity so `commit_all` works in CI.
    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        init(d.path()).unwrap();
        for kv in [
            ["user.email", "test@example.com"],
            ["user.name", "Test"],
            ["commit.gpgsign", "false"],
        ] {
            Command::new("git")
                .current_dir(d.path())
                .args(["config", kv[0], kv[1]])
                .output()
                .unwrap();
        }
        d
    }

    #[test]
    fn init_makes_a_repo() {
        if !git_available() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        assert!(!is_repo(d.path()));
        init(d.path()).unwrap();
        assert!(is_repo(d.path()));
    }

    #[test]
    fn remote_roundtrips() {
        if !git_available() {
            return;
        }
        let d = repo();
        assert!(remote_url(d.path(), "origin").is_none());
        set_remote(d.path(), "origin", "https://example.com/r.git").unwrap();
        assert_eq!(
            remote_url(d.path(), "origin").as_deref(),
            Some("https://example.com/r.git")
        );
        // set_remote updates an existing remote
        set_remote(d.path(), "origin", "https://example.com/other.git").unwrap();
        assert_eq!(
            remote_url(d.path(), "origin").as_deref(),
            Some("https://example.com/other.git")
        );
    }

    #[test]
    fn commit_all_reports_whether_it_committed() {
        if !git_available() {
            return;
        }
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "hi").unwrap();
        assert!(commit_all(d.path(), "first").unwrap());
        // nothing changed since
        assert!(!commit_all(d.path(), "noop").unwrap());
    }

    #[test]
    fn log_lines_traces_only_the_given_range() {
        if !git_available() {
            return;
        }
        let d = repo();
        let bib = d.path().join("references.bib");
        // Two entries; `a` on lines 1-3, `b` on lines 5-7.
        std::fs::write(&bib, "@misc{a,\n  t = {A}\n}\n\n@misc{b,\n  t = {B}\n}\n").unwrap();
        commit_all(d.path(), "add a,b").unwrap();
        // Change only `a`'s title; `b`'s lines are untouched.
        std::fs::write(&bib, "@misc{a,\n  t = {A2}\n}\n\n@misc{b,\n  t = {B}\n}\n").unwrap();
        commit_all(d.path(), "change a").unwrap();

        let a = log_lines(d.path(), "references.bib", 1, 3).unwrap();
        assert_eq!(
            a.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
            vec!["change a", "add a,b"], // newest first
        );
        assert!(!a[0].hash.is_empty() && !a[0].date.is_empty() && !a[0].author.is_empty());

        let b = log_lines(d.path(), "references.bib", 5, 7).unwrap();
        assert_eq!(
            b.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
            vec!["add a,b"],
        );
    }
}

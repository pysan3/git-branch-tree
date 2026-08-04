//! Fixture harness: build small real git repositories in tempdirs.
//!
//! Every integration test drives real git (CI has it); fixtures are milliseconds.
//!
//! The helpers are shared across test binaries, so not every one uses all of them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TestRepo {
    _tmp: tempfile::TempDir,
    pub dir: PathBuf,
}

impl TestRepo {
    /// `git init -b main` with a deterministic identity and one initial commit.
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path().join("repo");
        std::fs::create_dir(&dir).expect("create repo dir");
        let repo = Self { _tmp: tmp, dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.commit_file("README.md", "seed\n", "chore: initial commit");
        repo
    }

    /// Run git in the repo, panicking on failure (fixtures must not silently break).
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.dir, args)
    }

    /// Run git in an arbitrary directory (worktrees, remotes).
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    }

    /// Write `content` to `path` and commit it.
    pub fn commit_file(&self, path: &str, content: &str, msg: &str) {
        let full = self.dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&full, content).expect("write file");
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    /// Create `name` off `from` and check it out.
    pub fn branch_from(&self, name: &str, from: &str) {
        self.git(&["checkout", "-q", "-b", name, from]);
    }

    pub fn checkout(&self, name: &str) {
        self.git(&["checkout", "-q", name]);
    }

    /// Current commit id of a ref.
    pub fn sha(&self, rev: &str) -> String {
        self.git(&["rev-parse", rev])
    }

    /// Squash-merge `branch` into main (content lands under a brand-new commit).
    pub fn squash_merge_into_main(&self, branch: &str) {
        self.checkout("main");
        self.git(&["merge", "--squash", "-q", branch]);
        self.git(&["commit", "-q", "-m", &format!("squash: {branch}")]);
    }

    /// Add a bare clone as `origin` (for base-refresh tests).
    pub fn add_bare_origin(&self) -> PathBuf {
        let bare = self.dir.parent().unwrap().join("origin.git");
        let out = Command::new("git")
            .args([
                "clone",
                "-q",
                "--bare",
                self.dir.to_str().unwrap(),
                bare.to_str().unwrap(),
            ])
            .output()
            .expect("spawn git clone");
        assert!(out.status.success(), "bare clone failed");
        self.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        self.git(&["fetch", "-q", "origin"]);
        bare
    }
}

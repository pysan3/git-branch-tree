//! `--auto-merged`, driven against a stub `gh` on PATH so the tests stay offline.
//!
//! PATH is set per child process rather than with `set_var`, which would race across
//! the parallel test threads sharing one process environment.

mod common;

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use common::TestRepo;

/// Install a fake `gh` running `body`, and return the directory holding it.
fn stub_gh(r: &TestRepo, body: &str) -> PathBuf {
    let bin = r.dir.join("stubbin");
    std::fs::create_dir_all(&bin).unwrap();
    write_exe(&bin.join("gh"), &format!("#!/bin/sh\n{body}\n"));
    bin
}

fn write_exe(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// `bin` first, then the inherited PATH (so real git is still reachable).
fn path_with(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn stack() -> TestRepo {
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: b");
    r.checkout("main");
    r
}

fn gbt(r: &TestRepo, path: &str, args: &[&str]) -> Command {
    let mut cmd = Command::cargo_bin("git-branch-tree").unwrap();
    cmd.current_dir(&r.dir).env("PATH", path).arg("--no-fetch");
    cmd.args(args);
    cmd
}

#[test]
fn auto_merged_collapses_the_branch_and_notes_it() {
    let r = stack();
    // gh also reports a merged branch we did not ask about; it must be ignored.
    let bin = stub_gh(&r, "printf 'feat/a\\nsomeone-elses-branch\\n'");

    let out = gbt(
        &r,
        &path_with(&bin),
        &["--format", "ascii", "--auto-merged", "feat/a", "feat/b"],
    )
    .assert()
    .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.contains("# auto-detected as merged on GitHub: feat/a\n"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("someone-elses-branch"),
        "only analysed branches are reported:\n{stdout}"
    );
    // feat/a has landed, so it is gone from the tree and skipped in the rebase block.
    assert!(!stdout.contains("├─ feat/a"), "{stdout}");
    assert!(
        stdout.contains("# squash-merged branches skipped: feat/a"),
        "{stdout}"
    );
    assert!(stdout.contains("└─ feat/b"), "{stdout}");
}

#[test]
fn an_empty_pr_list_merges_nothing() {
    let r = stack();
    let bin = stub_gh(&r, "true");

    let out = gbt(
        &r,
        &path_with(&bin),
        &["--format", "ascii", "--auto-merged", "feat/a", "feat/b"],
    )
    .assert()
    .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();

    assert!(!stdout.contains("auto-detected as merged"), "{stdout}");
    assert!(
        !stdout.contains("squash-merged branches skipped"),
        "{stdout}"
    );
}

#[test]
fn without_the_flag_gh_is_never_consulted() {
    let r = stack();
    // A stub that would fail the run if it were called at all.
    let bin = stub_gh(&r, "echo 'gh should not run' >&2; exit 3");

    gbt(
        &r,
        &path_with(&bin),
        &["--format", "ascii", "feat/a", "feat/b"],
    )
    .assert()
    .success();
}

#[test]
fn a_failing_gh_is_reported_not_swallowed() {
    let r = stack();
    let bin = stub_gh(&r, "echo 'gh: not logged in' >&2; exit 1");

    let assert = gbt(&r, &path_with(&bin), &["--auto-merged", "feat/a", "feat/b"])
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.starts_with("error: gh pr list"), "{stderr}");
    assert!(stderr.contains("not logged in"), "{stderr}");
}

#[test]
fn a_missing_gh_explains_the_flag() {
    let r = stack();
    // A PATH holding only a git shim: no gh anywhere, but git still works.
    let bin = r.dir.join("nogh");
    std::fs::create_dir_all(&bin).unwrap();
    let real_git = which_git();
    write_exe(
        &bin.join("git"),
        &format!("#!/bin/sh\nexec {real_git} \"$@\"\n"),
    );

    let assert = gbt(
        &r,
        &bin.display().to_string(),
        &["--auto-merged", "feat/a", "feat/b"],
    )
    .assert()
    .failure()
    .code(1);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert_eq!(
        stderr,
        "error: gh (GitHub CLI) not found; install it or omit --auto-merged\n"
    );
}

/// Absolute path of the real git, so the shim above can forward to it.
fn which_git() -> String {
    let out = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

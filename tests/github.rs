//! `--auto-merged`, driven against a stub `gh` on PATH so the tests stay offline.
//!
//! PATH is set per child process rather than with `set_var`, which would race across
//! the parallel test threads sharing one process environment.
//!
//! Unix only: the stub is a `#!/bin/sh` script, which Windows will not execute. The
//! feature itself is portable - it just shells out to `gh` - so only the stubbing
//! technique is platform-bound.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::{Gbt, TestRepo, line_edit_chain};

/// Answers the per-branch `--head` query: a merged PR for feat/a, none for anything
/// else. Echoing the count is exactly what `--json number --jq length` produces.
const MERGED_A: &str = r#"case "$*" in
  *"--head feat/a "*) echo 1 ;;
  *) echo 0 ;;
esac"#;

/// Nothing is merged.
const MERGED_NONE: &str = "echo 0";

/// Satisfies preflight - it is installed and authenticated - then fails the actual
/// query, which is the case "a failing query must be surfaced" means to exercise.
const QUERY_FAILS: &str = r#"case "$1" in
  --version) echo "gh version 2.0.0" ;;
  auth) exit 0 ;;
  *) echo "gh: could not resolve repository" >&2; exit 1 ;;
esac"#;

/// Records every branch it was asked about, so a test can assert the tool asks about
/// the branches it is analysing rather than pulling a bulk list.
const RECORD_QUERIES: &str = r#"for a in "$@"; do
  case "$prev" in --head) echo "$a" >> "$GBT_QUERY_LOG" ;; esac
  prev="$a"
done
echo 0"#;

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

/// feat/a and feat/b of the shared chain; feat/c is unused here.
fn stack() -> TestRepo {
    line_edit_chain()
}

fn gbt(r: &TestRepo, path: &str, args: &[&str]) -> Gbt {
    Gbt::new(r).env("PATH", path).args(args)
}

#[test]
fn auto_merged_collapses_the_branch_and_notes_it() {
    let r = stack();
    let bin = stub_gh(&r, MERGED_A);

    let stdout = gbt(
        &r,
        &path_with(&bin),
        &["--format", "ascii", "--auto-merged", "feat/a", "feat/b"],
    )
    .stdout();

    assert!(
        stdout.contains("# auto-detected as merged on GitHub: feat/a\n"),
        "{stdout}"
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
fn finding_nothing_is_reported_rather_than_silent() {
    let r = stack();
    let bin = stub_gh(&r, MERGED_NONE);

    let (stdout, stderr) = gbt(
        &r,
        &path_with(&bin),
        &["--format", "ascii", "--auto-merged", "feat/a", "feat/b"],
    )
    .output();

    assert!(!stdout.contains("auto-detected as merged"), "{stdout}");
    assert!(
        !stdout.contains("squash-merged branches skipped"),
        "{stdout}"
    );
    // Silence here is indistinguishable from the flag never running - which is exactly
    // how a real bug in this code went unnoticed.
    assert!(
        stderr.contains("asking GitHub whether 2 branch(es) have merged pull requests"),
        "{stderr}"
    );
    assert!(
        stderr.contains("no merged pull requests found for the analysed branches"),
        "{stderr}"
    );
}

#[test]
fn each_analysed_branch_is_queried_directly() {
    // Regression test. This used to pull the last 1000 merged PRs and intersect, which
    // made the cost scale with the repository's activity instead of the branch count:
    // on a busy monorepo the window covered days, so every branch merged before it was
    // silently missed. Each branch must be asked about by name.
    let r = stack();
    let log = r.dir.join("queries.txt");
    let bin = stub_gh(&r, RECORD_QUERIES);

    gbt(
        &r,
        &path_with(&bin),
        &["--format", "ascii", "--auto-merged", "feat/a", "feat/b"],
    )
    .env("GBT_QUERY_LOG", log.to_str().unwrap())
    .stdout();

    let mut asked: Vec<String> = std::fs::read_to_string(&log)
        .expect("the stub recorded its queries")
        .lines()
        .map(str::to_string)
        .collect();
    asked.sort();
    assert_eq!(
        asked,
        vec!["feat/a".to_string(), "feat/b".to_string()],
        "every analysed branch should be queried by name"
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
    .stdout();
}

#[test]
fn a_failing_gh_is_reported_not_swallowed() {
    let r = stack();
    let bin = stub_gh(&r, QUERY_FAILS);

    let stderr = gbt(&r, &path_with(&bin), &["--auto-merged", "feat/a", "feat/b"]).failure();
    assert!(stderr.contains("error: gh pr list"), "{stderr}");
    assert!(stderr.contains("could not resolve repository"), "{stderr}");
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

    let stderr = gbt(
        &r,
        &bin.display().to_string(),
        &["--auto-merged", "feat/a", "feat/b"],
    )
    .failure();
    assert!(
        stderr.contains("--auto-merged needs the gh CLI, which was not found"),
        "{stderr}"
    );
    assert!(stderr.contains("or drop --auto-merged"), "{stderr}");
}

/// Absolute path of the real git, so the shim above can forward to it.
fn which_git() -> String {
    let out = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

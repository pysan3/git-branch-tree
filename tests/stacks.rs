//! `--from-gh-stack`, driven against a stub `gh` on PATH so the tests stay offline.
//!
//! The stub is what makes these worth writing: it pins the *real* argv the tool sends,
//! which a fake in Rust would stop proving. PATH is set per child process rather than
//! with `set_var`, which would race the parallel test threads sharing one environment.
//!
//! Unix only, for the `#!/bin/sh` stub. The feature itself is portable.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::{Gbt, TestRepo, line_edit_chain};

/// A working `gh` with the stack extension: `gh stack --help` succeeds (preflight) and
/// `gh stack view --short` lists two of the three branches in the fixture.
const STACK_AB: &str = r#"case "$*" in
  "stack --help") echo "Work with stacks" ;;
  "stack view --short") printf 'feat/a\nfeat/b\n' ;;
  *) exit 1 ;;
esac"#;

/// `gh` is installed but the stack extension is not, so `gh stack` fails while
/// `gh --version` succeeds - the case a naive `--version` probe would wave through.
const NO_EXTENSION: &str = r#"case "$1" in
  --version) echo "gh version 2.62.0" ;;
  stack) echo "unknown command \"stack\" for \"gh\"" >&2; exit 1 ;;
  *) exit 1 ;;
esac"#;

/// Passes preflight, then names a branch that does not exist here alongside two that do.
const STACK_WITH_STRAY: &str = r#"case "$*" in
  "stack --help") echo "Work with stacks" ;;
  "stack view --short") printf 'feat/a\nfeat/gone\nfeat/b\n' ;;
  *) exit 1 ;;
esac"#;

/// Passes preflight, then names nothing this repository knows - what a rendering change
/// on the other side would look like.
const STACK_ALL_STRAY: &str = r#"case "$*" in
  "stack --help") echo "Work with stacks" ;;
  "stack view --short") printf 'nope/one\nnope/two\n' ;;
  *) exit 1 ;;
esac"#;

/// Passes preflight, then prints nothing: not on a stack.
const STACK_EMPTY: &str = r#"case "$*" in
  "stack --help") echo "Work with stacks" ;;
  "stack view --short") : ;;
  *) exit 1 ;;
esac"#;

/// Records the full argv it was invoked with, so a test can assert the exact command.
const RECORD_ARGV: &str = r#"echo "$*" >> "$GBT_ARGV_LOG"
case "$*" in
  "stack --help") echo "Work with stacks" ;;
  "stack view --short") printf 'feat/a\nfeat/b\n' ;;
esac"#;

fn stub_gh(r: &TestRepo, body: &str) -> PathBuf {
    let bin = r.dir.join("stubbin");
    std::fs::create_dir_all(&bin).unwrap();
    write_exe(&bin.join("gh"), &format!("#!/bin/sh\n{body}\n"));
    bin
}

fn write_exe(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `bin` first, then the inherited PATH (so real git is still reachable).
fn path_with(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn gbt(r: &TestRepo, bin: &Path, args: &[&str]) -> Gbt {
    Gbt::new(r).env("PATH", path_with(bin)).args(args)
}

#[test]
fn the_stack_supplies_the_branch_list() {
    let r = line_edit_chain();
    let bin = stub_gh(&r, STACK_AB);

    let (stdout, stderr) = gbt(&r, &bin, &["--format", "ascii", "--from-gh-stack"]).output();

    // feat/c is in the repository but not in the stack, so it must not be analysed.
    assert!(
        stdout.contains("# branches (2): feat/a, feat/b"),
        "{stdout}"
    );
    assert!(!stdout.contains("feat/c"), "{stdout}");
    assert!(
        stderr.contains("--from-gh-stack: 2 branch(es) from `gh stack view --short`"),
        "{stderr}"
    );
}

#[test]
fn the_declared_order_is_read_but_the_edges_are_not() {
    // The whole point of the flag: gh stack says b is stacked on a, but they edit the
    // same line, so the content engine has to reach that conclusion on its own - and it
    // would reach a different one if they did not.
    let r = line_edit_chain();
    let bin = stub_gh(&r, STACK_AB);

    let stdout = gbt(&r, &bin, &["--format", "ascii", "--from-gh-stack"]).stdout();

    assert!(stdout.contains("└─ feat/a"), "{stdout}");
    assert!(stdout.contains("   └─ feat/b"), "{stdout}");
}

#[test]
fn the_extension_is_probed_rather_than_just_gh() {
    // `gh --version` succeeds here. Probing that instead of `gh stack` would pass
    // preflight and then fail on the one command that matters.
    let r = line_edit_chain();
    let bin = stub_gh(&r, NO_EXTENSION);

    let stderr = gbt(&r, &bin, &["--from-gh-stack"]).failure();

    assert!(stderr.contains("unmet requirement"), "{stderr}");
    assert!(stderr.contains("`gh stack --help`"), "{stderr}");
    assert!(
        stderr.contains("gh extension install github/gh-stack"),
        "{stderr}"
    );
}

#[test]
fn a_missing_gh_is_reported_by_preflight() {
    let r = line_edit_chain();
    // An empty stub dir plus a git shim, so PATH holds no gh at all.
    let bin = r.dir.join("emptybin");
    std::fs::create_dir_all(&bin).unwrap();
    let real_git = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    write_exe(
        &bin.join("git"),
        &format!("#!/bin/sh\nexec {} \"$@\"\n", real_git.trim()),
    );

    let stderr = Gbt::new(&r)
        .env("PATH", bin.to_str().unwrap())
        .args(&["--from-gh-stack"])
        .failure();

    assert!(
        stderr.contains("--from-gh-stack needs `gh stack --help`"),
        "{stderr}"
    );
}

#[test]
fn a_branch_the_repository_does_not_have_is_skipped_with_a_warning() {
    let r = line_edit_chain();
    let bin = stub_gh(&r, STACK_WITH_STRAY);

    let (stdout, stderr) = gbt(&r, &bin, &["--format", "ascii", "--from-gh-stack"]).output();

    // Deleting a branch locally must not sink the whole run.
    assert!(
        stdout.contains("# branches (2): feat/a, feat/b"),
        "{stdout}"
    );
    assert!(
        stderr.contains("--from-gh-stack: not a local branch, skipped: feat/gone"),
        "{stderr}"
    );
}

#[test]
fn output_naming_no_local_branch_says_the_parse_went_wrong() {
    // Distinguishing this from "you have no stack" is the point: if the other project
    // changes its renderer, the message has to say so rather than report an empty tree.
    let r = line_edit_chain();
    let bin = stub_gh(&r, STACK_ALL_STRAY);

    let stderr = gbt(&r, &bin, &["--from-gh-stack"]).failure();

    assert!(
        stderr.contains("none of the 2 name(s) from `gh stack view --short` is a local branch"),
        "{stderr}"
    );
    assert!(stderr.contains("nope/one, nope/two"), "{stderr}");
}

#[test]
fn an_empty_stack_is_an_error_rather_than_an_empty_report() {
    let r = line_edit_chain();
    let bin = stub_gh(&r, STACK_EMPTY);

    let stderr = gbt(&r, &bin, &["--from-gh-stack"]).failure();

    assert!(
        stderr.contains("`gh stack view --short` named no branches"),
        "{stderr}"
    );
}

#[test]
fn exactly_one_listing_command_is_issued() {
    // Pins the real argv. The preflight probe and the listing are the only calls, and
    // neither may quietly become a network-bound one.
    let r = line_edit_chain();
    let bin = stub_gh(&r, RECORD_ARGV);
    let log = r.dir.join("argv.log");

    Gbt::new(&r)
        .env("PATH", path_with(&bin))
        .env("GBT_ARGV_LOG", log.to_str().unwrap())
        .args(&["--format", "ascii", "--from-gh-stack"])
        .stdout();

    let calls: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(calls, vec!["stack --help", "stack view --short"]);
}

#[test]
fn the_flag_cannot_be_combined_with_the_other_input_modes() {
    let r = line_edit_chain();
    let bin = stub_gh(&r, STACK_AB);

    gbt(&r, &bin, &["--from-gh-stack", "--prefix", "feat"]).any_failure();
    gbt(&r, &bin, &["feat/a", "--from-gh-stack"]).any_failure();
}

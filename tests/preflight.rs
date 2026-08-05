//! Runtime dependency checks.
//!
//! Unix only: the stubs are `#!/bin/sh` scripts. The checks themselves are portable.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::{Gbt, TestRepo, disjoint_stack};

fn write_exe(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A directory containing a stub `gh` (and a `git` shim, so PATH can be replaced
/// wholesale without losing git).
fn stub_dir(r: &TestRepo, name: &str, gh_body: Option<&str>) -> PathBuf {
    let bin = r.dir.join(name);
    std::fs::create_dir_all(&bin).unwrap();
    let real_git = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    write_exe(
        &bin.join("git"),
        &format!("#!/bin/sh\nexec {real_git} \"$@\"\n"),
    );
    if let Some(body) = gh_body {
        write_exe(&bin.join("gh"), &format!("#!/bin/sh\n{body}\n"));
    }
    bin
}

const GH_OK: &str = r#"case "$1" in
  --version) echo "gh version 2.0.0" ;;
  auth) exit 0 ;;
  *) echo 0 ;;
esac"#;

const GH_UNAUTHENTICATED: &str = r#"case "$1" in
  --version) echo "gh version 2.0.0" ;;
  auth) echo "not logged in" >&2; exit 1 ;;
  *) exit 1 ;;
esac"#;

#[test]
fn a_missing_gh_is_reported_with_a_remedy() {
    let r = disjoint_stack();
    let bin = stub_dir(&r, "nogh", None);

    let stderr = Gbt::new(&r)
        .env("PATH", bin.to_str().unwrap())
        .args(&["--auto-merged", "feat/a", "feat/b"])
        .failure();
    assert!(
        stderr.contains("--auto-merged needs the gh CLI, which was not found"),
        "{stderr}"
    );
    // The remedy is the useful half.
    assert!(stderr.contains("install gh"), "{stderr}");
    assert!(stderr.contains("or drop --auto-merged"), "{stderr}");
}

#[test]
fn an_unauthenticated_gh_is_caught_before_any_query() {
    // Installed is not the same as usable: without this the first query fails instead,
    // after the base has been refreshed and every branch resolved.
    let r = disjoint_stack();
    let bin = stub_dir(&r, "unauth", Some(GH_UNAUTHENTICATED));

    let stderr = Gbt::new(&r)
        .env("PATH", bin.to_str().unwrap())
        .args(&["--auto-merged", "feat/a", "feat/b"])
        .failure();
    assert!(
        stderr.contains("--auto-merged needs gh to be authenticated"),
        "{stderr}"
    );
    assert!(stderr.contains("gh auth login"), "{stderr}");
}

#[test]
fn the_checks_run_before_the_branches_are_resolved() {
    // The point of the whole module. These branch names do not exist, so if the checks
    // ran later the error would be "branch 'ghost' does not exist" - reached only after
    // the base refresh and a patch-id pass. Seeing the gh complaint proves the ordering.
    let r = disjoint_stack();
    let bin = stub_dir(&r, "nogh2", None);

    let stderr = Gbt::new(&r)
        .env("PATH", bin.to_str().unwrap())
        .args(&["--auto-merged", "ghost-one", "ghost-two"])
        .failure();
    assert!(stderr.contains("needs the gh CLI"), "{stderr}");
    assert!(
        !stderr.contains("does not exist"),
        "branch resolution should not have been reached:\n{stderr}"
    );
}

#[test]
fn every_problem_is_reported_at_once() {
    // Fixing one only to be told about the next is a poor trade when reaching the
    // failure is slow.
    let r = disjoint_stack();
    let bin = stub_dir(&r, "multi", None);

    let stderr = Gbt::new(&r)
        .env("PATH", bin.to_str().unwrap())
        .args(&[
            "--auto-merged",
            "--test",
            "true",
            "--test-patch",
            "/nonexistent/fix.patch",
            "feat/a",
        ])
        .failure();
    assert!(stderr.contains("unmet requirements:"), "plural:\n{stderr}");
    assert!(stderr.contains("needs the gh CLI"), "{stderr}");
    assert!(
        stderr.contains("--test-patch file not found"),
        "both problems, not just the first:\n{stderr}"
    );
}

#[test]
fn a_healthy_environment_passes_quietly() {
    let r = disjoint_stack();
    let bin = stub_dir(&r, "ok", Some(GH_OK));

    let (stdout, stderr) = Gbt::new(&r)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .args(&["--format", "ascii", "--auto-merged", "feat/a", "feat/b"])
        .output();
    assert!(stdout.contains("# base: main"), "{stdout}");
    assert!(!stderr.contains("unmet requirement"), "{stderr}");
}

#[test]
fn a_missing_origin_warns_rather_than_refusing() {
    // The analysis still works against the local base, so this must not be fatal - the
    // fixtures have no origin at all, which is the common offline case.
    let r = disjoint_stack();

    // Gbt always passes --no-fetch, so ask for a fetch explicitly by not using it.
    let out = assert_cmd::Command::cargo_bin("git-branch-tree")
        .unwrap()
        .current_dir(&r.dir)
        .args(["--format", "ascii", "--no-rebase", "feat/a", "feat/b"])
        .assert()
        .success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("no 'origin' remote"),
        "should warn about the missing remote:\n{stderr}"
    );
    assert!(
        stderr.contains("--no-fetch"),
        "and suggest the flag:\n{stderr}"
    );
}

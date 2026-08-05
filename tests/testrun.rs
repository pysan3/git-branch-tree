//! `--test`: real rebases in real worktrees, running real commands.
//!
//! Only branches that would land on the base are tested, and only branches with a
//! chain-upstream appear in the plan at all - so the fixtures here are stacks whose
//! members turn out to be content-independent, which is exactly the case the tool
//! flattens.
//!
//! Unix only: the test commands here are POSIX (`true`, `test ! -f`). The runner itself
//! is portable and dispatches through `cmd /C` on Windows; it is these fixtures that
//! would need a second dialect.
#![cfg(unix)]

mod common;

use common::{Gbt, TestRepo, disjoint_stack as chain};

fn run(r: &TestRepo, fixed: &[&str], branches: &[&str]) -> String {
    Gbt::new(r).args(fixed).args(branches).stdout()
}

const CHAIN: &[&str] = &["feat/a", "feat/b", "feat/c"];

#[test]
fn a_passing_command_leaves_every_tested_branch_in_the_chain() {
    let r = chain();
    let out = run(&r, &["--format", "ascii", "--test", "true"], CHAIN);
    assert!(out.contains("git checkout feat/b"), "{out}");
    assert!(out.contains("git checkout feat/c"), "{out}");
    assert!(!out.contains("left alone"), "{out}");
}

#[test]
fn a_failing_command_drops_that_branch_and_says_why() {
    let r = chain();
    // Fails only in the worktree that actually contains b.txt, i.e. for feat/b.
    let out = run(
        &r,
        &["--format", "ascii", "--test", "test ! -f b.txt"],
        CHAIN,
    );
    assert!(
        out.contains("# left alone (failed --test, or stacked on a failed branch; rerun later):"),
        "{out}"
    );
    assert!(
        out.contains("#   feat/b  (test command failed (exit 1))"),
        "{out}"
    );
    // The branch whose test passed is still rebased.
    assert!(out.contains("git checkout feat/c"), "{out}");
    assert!(!out.contains("git checkout feat/b"), "{out}");
}

#[test]
fn the_command_sees_the_base_plus_only_that_branch() {
    // Testing feat/b alone: its worktree must hold the base and b.txt, but neither its
    // ancestor's a.txt (dropped by the rebase) nor its descendant's c.txt.
    let r = chain();
    let out = run(
        &r,
        &[
            "--format",
            "ascii",
            "--test",
            "test -f README.md && test -f b.txt && test ! -f a.txt && test ! -f c.txt",
        ],
        &["feat/a", "feat/b"],
    );
    assert!(out.contains("git checkout feat/b"), "{out}");
    assert!(!out.contains("left alone"), "{out}");
}

#[test]
fn a_branch_stacked_on_a_failure_is_left_alone_too() {
    // feat/c edits the line feat/b introduced, so it lands on feat/b and is never tested
    // itself. If feat/b fails, feat/c's code is unproven, so it must not be moved either.
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "b1\nb2\nb3\n", "feat: b adds f.txt");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("f.txt", "C1\nb2\nb3\n", "feat: c edits b's line");
    r.checkout("main");

    let out = run(&r, &["--format", "ascii", "--test", "exit 7"], CHAIN);
    assert!(
        out.contains("#   feat/b  (test command failed (exit 7))"),
        "{out}"
    );
    assert!(
        out.contains("#   feat/c  (stacked on feat/b which failed --test)"),
        "{out}"
    );
    assert!(
        out.contains("# (nothing to rebase - every branch already sits on the base)"),
        "{out}"
    );
}

#[test]
fn worktrees_live_at_a_predictable_path_reused_across_runs() {
    let r = chain();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let (_, stderr) = Gbt::new(&r)
            .args(&["--format", "ascii", "--test", "true"])
            .args(CHAIN)
            .output();
        seen.push(
            stderr
                .lines()
                .find(|l| l.starts_with("# test worktrees under "))
                .expect("the runner announces its worktree root")
                .to_string(),
        );
    }
    assert_eq!(
        seen[0], seen[1],
        "the same root is reused, keeping caches warm"
    );
    assert!(seen[0].contains("git-branch-tree"), "{}", seen[0]);
}

#[test]
fn test_jobs_one_still_tests_everything() {
    let r = chain();
    let out = run(
        &r,
        &[
            "--format",
            "ascii",
            "--test",
            "test ! -f b.txt",
            "--test-jobs",
            "1",
        ],
        CHAIN,
    );
    assert!(out.contains("#   feat/b  (test command failed"), "{out}");
    assert!(out.contains("git checkout feat/c"), "{out}");
}

#[test]
fn a_test_patch_is_applied_before_the_command() {
    let r = chain();
    // The command insists on a file no branch contains; only the patch can provide it.
    let patch = r.dir.parent().unwrap().join("fix.patch");
    std::fs::write(
        &patch,
        "diff --git a/fix.txt b/fix.txt\n\
         new file mode 100644\n\
         index 0000000..257cc56\n\
         --- /dev/null\n\
         +++ b/fix.txt\n\
         @@ -0,0 +1 @@\n\
         +fixed\n",
    )
    .unwrap();

    let out = run(
        &r,
        &[
            "--format",
            "ascii",
            "--test",
            "test -f fix.txt",
            "--test-patch",
            patch.to_str().unwrap(),
        ],
        CHAIN,
    );
    assert!(
        !out.contains("left alone"),
        "the patch should make the command pass:\n{out}"
    );
    assert!(out.contains("git checkout feat/b"), "{out}");
}

#[test]
fn a_missing_test_patch_is_rejected_up_front() {
    let r = chain();
    assert_eq!(
        Gbt::new(&r)
            .args(&[
                "--test",
                "true",
                "--test-patch",
                "/nonexistent/fix.patch",
                "feat/a",
                "feat/b",
            ])
            .failure(),
        "error: unmet requirement:\n  - --test-patch file not found: /nonexistent/fix.patch\n    give a path to an existing patch file\n"
    );
}

#[test]
fn a_branch_that_cannot_rebase_cleanly_is_reported_as_such() {
    // feat/c's own commit rewrites a line that only exists on feat/b, so replaying it
    // straight onto the base conflicts - which is the tool discovering a real dependency
    // its heuristics did not see.
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "l1\nB2\nl3\n", "feat: b rewrites line 2");
    r.checkout("main");
    // Give the base a conflicting edit to the same line.
    r.commit_file("f.txt", "l1\nMAIN2\nl3\n", "chore: base moves line 2");

    let out = run(
        &r,
        &["--format", "ascii", "--test", "true"],
        &["feat/a", "feat/b"],
    );
    assert!(
        out.contains("#   feat/b  (does not apply cleanly onto the base)"),
        "{out}"
    );
}

#[test]
fn without_the_flag_no_worktrees_are_made() {
    let r = chain();
    let (_, stderr) = Gbt::new(&r)
        .args(&["--format", "ascii"])
        .args(CHAIN)
        .output();
    assert!(!stderr.contains("test worktrees under"), "{stderr}");
}

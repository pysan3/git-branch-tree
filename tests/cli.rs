//! End-to-end runs of the actual binary: the report as a user sees it.

mod common;

use assert_cmd::Command;
use common::TestRepo;

/// Run the binary in `r` with `--no-fetch` (fixtures have no origin) and return stdout.
fn run(r: &TestRepo, args: &[&str]) -> String {
    let out = Command::cargo_bin("git-branch-tree")
        .unwrap()
        .current_dir(&r.dir)
        .arg("--no-fetch")
        .args(args)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    normalise(&stdout)
}

/// Replace the 10-hex short shas in the rebase block, which change whenever the
/// fixture's content does, so the assertions stay about structure.
fn normalise(s: &str) -> String {
    s.split(' ')
        .map(|tok| {
            if tok.len() == 10 && tok.bytes().all(|b| b.is_ascii_hexdigit()) {
                "<SHA>"
            } else {
                tok
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A genuine diamond: root owns line 2, left owns line 1, right owns line 3, and top
/// edits lines 1 and 3 so it depends on both left and right.
fn diamond() -> TestRepo {
    let r = TestRepo::new();
    r.commit_file("f.txt", "r1\nr2\nr3\n", "chore: seed");
    r.branch_from("feat/root", "main");
    r.commit_file("f.txt", "r1\nR2\nr3\n", "feat: root");
    r.branch_from("feat/left", "feat/root");
    r.commit_file("f.txt", "L1\nR2\nr3\n", "feat: left");
    r.branch_from("feat/right", "feat/left");
    r.commit_file("f.txt", "L1\nR2\nG3\n", "feat: right");
    r.branch_from("feat/top", "feat/right");
    r.commit_file("f.txt", "T1\nR2\nT3\n", "feat: top");
    r.checkout("main");
    r
}

const ALL: &[&str] = &["feat/root", "feat/left", "feat/right", "feat/top"];

#[test]
fn reports_tree_and_rebase_block() {
    let r = diamond();
    let mut args = vec!["--format", "ascii"];
    args.extend_from_slice(ALL);
    assert_eq!(
        run(&r, &args),
        "\
# base: main
# branches (4): feat/root, feat/left, feat/right, feat/top

main
├─ feat/root
├─ feat/left
└─ feat/right
   └─ feat/top   (also depends on: feat/left)

true \\
&& git rebase --onto main <SHA> feat/left && git checkout feat/left && git push --force-with-lease && review \\
&& git rebase --onto main <SHA> feat/right && git checkout feat/right && git push --force-with-lease && review \\
&& git rebase --onto feat/right <SHA> feat/top && git checkout feat/top && git push --force-with-lease && gh pr edit feat/top --base feat/right \\
&& true
"
    );
}

#[test]
fn mermaid_is_the_default_and_both_prints_two_trees() {
    let r = diamond();
    let out = run(&r, ALL);
    assert!(
        out.contains("```mermaid"),
        "default should be mermaid:\n{out}"
    );
    assert!(
        !out.contains("├─"),
        "default should not print ascii:\n{out}"
    );

    let mut args = vec!["--format", "both"];
    args.extend_from_slice(ALL);
    let out = run(&r, &args);
    assert!(out.contains("```mermaid") && out.contains("├─"));
}

#[test]
fn no_rebase_omits_the_command_block() {
    let r = diamond();
    let mut args = vec!["--no-rebase", "--format", "ascii"];
    args.extend_from_slice(ALL);
    let out = run(&r, &args);
    assert!(out.contains("main\n├─ feat/root"));
    assert!(!out.contains("git rebase --onto"), "{out}");
}

#[test]
fn merged_branches_are_skipped_and_noted() {
    let r = diamond();
    // `--merged` takes one-or-more values, so it goes last or it would swallow the
    // positional branches (matching the original's argparse behaviour).
    let mut args = vec!["--format", "ascii"];
    args.extend_from_slice(ALL);
    args.extend_from_slice(&["--merged", "feat/left"]);
    let out = run(&r, &args);
    assert!(
        out.contains("# squash-merged branches skipped: feat/left"),
        "{out}"
    );
    // The merged branch is gone from the tree and from the commands.
    assert!(!out.contains("├─ feat/left"), "{out}");
    assert!(!out.contains("git checkout feat/left"), "{out}");
}

#[test]
fn a_single_branch_pulls_in_its_whole_stack() {
    let r = diamond();
    let out = run(&r, &["--format", "ascii", "feat/root"]);
    // Everything stacked on root by content is discovered without naming it.
    assert!(
        out.starts_with(
            "# base: main\n# branches (4): feat/left, feat/right, feat/root, feat/top\n"
        ),
        "{out}"
    );
}

#[test]
fn ancestry_mode_reports_the_literal_git_stack() {
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b\n", "feat: b");
    r.checkout("main");

    // Content says independent...
    let out = run(
        &r,
        &["--format", "ascii", "--no-rebase", "feat/a", "feat/b"],
    );
    assert!(out.contains("main\n├─ feat/a\n└─ feat/b"), "{out}");

    // ...ancestry says stacked.
    let out = run(
        &r,
        &[
            "--format",
            "ascii",
            "--no-rebase",
            "--ancestry",
            "feat/a",
            "feat/b",
        ],
    );
    assert!(out.contains("main\n└─ feat/a\n   └─ feat/b"), "{out}");
}

#[test]
fn nothing_to_rebase_says_so() {
    let r = TestRepo::new();
    r.branch_from("feat/only", "main");
    r.commit_file("only.txt", "only\n", "feat: only");
    r.checkout("main");

    let out = run(&r, &["--format", "ascii", "feat/only"]);
    assert!(
        out.contains("# (nothing to rebase - every branch already sits on the base)"),
        "{out}"
    );
}

#[test]
fn errors_go_to_stderr_with_exit_one() {
    let r = diamond();

    let assert = Command::cargo_bin("git-branch-tree")
        .unwrap()
        .current_dir(&r.dir)
        .args(["--no-fetch", "ghost"])
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert_eq!(stderr, "error: branch 'ghost' does not exist\n");

    let assert = Command::cargo_bin("git-branch-tree")
        .unwrap()
        .current_dir(&r.dir)
        .arg("--no-fetch")
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert_eq!(
        stderr,
        "error: no branches given; pass branch name(s) or --prefix\n"
    );
}

#[test]
fn suffix_commands_are_configurable() {
    let r = diamond();

    // Replacing both sides: the templates expand per branch and landing kind.
    let out = run(
        &r,
        &[
            "--format",
            "ascii",
            "--no-rebase",
            "feat/root",
            "--on-base",
            "unused",
        ],
    );
    assert!(out.contains("# base: main"), "{out}");

    let mut args = vec!["--format", "ascii"];
    args.extend_from_slice(ALL);
    args.extend_from_slice(&[
        "--on-base",
        "ship {branch} onto {base}",
        "--on-parent",
        "retarget {branch} -> {onto}",
    ]);
    let out = run(&r, &args);
    assert!(
        out.contains("&& git push --force-with-lease && ship feat/left onto main \\"),
        "{out}"
    );
    assert!(
        out.contains("&& git push --force-with-lease && retarget feat/top -> feat/right \\"),
        "{out}"
    );
    // The defaults are gone, not appended to.
    assert!(!out.contains("&& review"), "{out}");
    assert!(!out.contains("gh pr edit"), "{out}");
}

#[test]
fn repeating_a_suffix_flag_chains_several_commands() {
    let r = diamond();
    let mut args = vec!["--format", "ascii"];
    args.extend_from_slice(ALL);
    args.extend_from_slice(&["--on-base", "first {branch}", "--on-base", "second {up}"]);
    let out = run(&r, &args);
    assert!(
        out.contains("&& first feat/left && second <SHA> \\"),
        "{out}"
    );
}

#[test]
fn an_empty_suffix_appends_nothing() {
    let r = diamond();
    let mut args = vec!["--format", "ascii"];
    args.extend_from_slice(ALL);
    args.extend_from_slice(&["--on-base", "", "--on-parent", ""]);
    let out = run(&r, &args);
    assert!(
        out.contains("&& git rebase --onto main <SHA> feat/left && git checkout feat/left && git push --force-with-lease \\"),
        "{out}"
    );
    assert!(!out.contains("&& review"), "{out}");
}

#[test]
fn a_bad_placeholder_is_rejected_before_any_git_work() {
    let r = diamond();
    let assert = Command::cargo_bin("git-branch-tree")
        .unwrap()
        .current_dir(&r.dir)
        .args(["--no-fetch", "feat/root", "--on-base", "echo {nope}"])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("unknown placeholder '{nope}'"), "{stderr}");
    assert!(
        stderr.contains("{branch}, {onto}, {base}, {up}"),
        "{stderr}"
    );
}

#[test]
fn braces_can_be_escaped_in_a_suffix() {
    let r = diamond();
    let mut args = vec!["--format", "ascii"];
    args.extend_from_slice(ALL);
    args.extend_from_slice(&["--on-base", "jq '{{n: 1}}' # {branch}"]);
    let out = run(&r, &args);
    assert!(out.contains("&& jq '{n: 1}' # feat/left \\"), "{out}");
}

#[test]
fn a_hostile_branch_name_cannot_inject_shell_commands() {
    // git allows `;` and `$(...)` in ref names, and the rebase block is meant to be
    // pasted into a shell - so a branch fetched from an untrusted fork must not be able
    // to smuggle commands into it.
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/base", "main");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: base");
    r.branch_from("feat/x;id", "feat/base");
    r.commit_file("f.txt", "X1\nl2\nl3\n", "feat: evil");
    r.checkout("main");

    let out = run(&r, &["--format", "ascii", "feat/base", "feat/x;id"]);
    // The name reaches every command as one quoted literal, never as shell syntax.
    // (util::shell_quote's own tests prove the round trip against a real shell.)
    assert!(out.contains("git checkout 'feat/x;id'"), "{out}");
    assert!(
        !out.contains("git checkout feat/x;id"),
        "an unquoted name would run `id` on paste:\n{out}"
    );
    assert!(
        !out.contains("gh pr edit feat/x;id"),
        "the suffix command must quote it too:\n{out}"
    );
    // The tree is not shell, so it shows the plain name.
    assert!(out.contains("└─ feat/x;id"), "{out}");
}

#[test]
fn runs_from_a_subdirectory() {
    // The tool discovers the repository upward, so it works anywhere inside it.
    let r = diamond();
    let sub = r.dir.join("nested/deeper");
    std::fs::create_dir_all(&sub).unwrap();

    let out = Command::cargo_bin("git-branch-tree")
        .unwrap()
        .current_dir(&sub)
        .args([
            "--no-fetch",
            "--format",
            "ascii",
            "--no-rebase",
            "feat/root",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("# base: main"), "{stdout}");
}

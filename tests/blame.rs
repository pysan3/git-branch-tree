//! Bounded blame against real git, plus the hunk parsing that feeds it.
//!
//! These two must agree with git exactly: the `--unified=0` hunk boundaries decide
//! which lines get blamed, and a one-line drift would attribute a line to the wrong
//! commit and flip a dependency edge.

mod common;

use common::TestRepo;
use git_branch_tree::blame::{Blamer, SubprocessBlamer};
use git_branch_tree::gitx::{Git, RepoView, Sha};
use git_branch_tree::hunks::{diff_unified0, parse_old_side_hunks};

fn blamer(r: &TestRepo) -> SubprocessBlamer {
    SubprocessBlamer {
        git: Git::new(&r.dir),
    }
}

fn sha(r: &TestRepo, rev: &str) -> Sha {
    r.sha(rev).parse().unwrap()
}

/// main holds a 3-line file; feat/a rewrites line 2; feat/b then rewrites line 3.
///
/// So feat/b edits a line the *base* owns, not one feat/a introduced - which is the
/// case that must NOT produce a dependency edge.
fn edited_repo() -> TestRepo {
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed file");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "l1\nA2\nl3\n", "feat: rewrite line 2");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "l1\nA2\nB3\n", "feat: rewrite line 3");
    r.checkout("main");
    r
}

#[test]
fn blame_attributes_lines_to_the_introducing_commit() {
    let r = edited_repo();
    let b = blamer(&r);
    let a_commit = sha(&r, "feat/a");
    let b_commit = sha(&r, "feat/b");

    // Line 2 of feat/b was last touched by feat/a's commit.
    let got = b.blame_range("feat/b", "main", "f.txt", 2, 2);
    assert!(
        got.contains(&a_commit),
        "line 2 should blame to feat/a's commit, got {got:?}"
    );
    assert!(!got.contains(&b_commit));

    // Line 3 was last touched by feat/b's own commit.
    let got = b.blame_range("feat/b", "main", "f.txt", 3, 3);
    assert!(got.contains(&b_commit), "line 3 should blame to feat/b");
}

#[test]
fn blame_is_bounded_by_the_base_range() {
    let r = edited_repo();
    let b = blamer(&r);
    let seed = sha(&r, "main");

    // Line 1 originates at or before the base. Bounded to main..feat/b it is reported
    // against a boundary commit, so the seed commit itself must not surface as a
    // dependency - that is what keeps blame off the base's full history.
    let got = b.blame_range("feat/b", "main", "f.txt", 1, 1);
    assert!(
        !got.contains(&seed),
        "base-side commit must not be attributed, got {got:?}"
    );
}

#[test]
fn out_of_range_and_missing_inputs_yield_no_edges() {
    let r = edited_repo();
    let b = blamer(&r);

    // Ranges past EOF are clamped to the file rather than failing the run.
    let got = b.blame_range("feat/b", "main", "f.txt", 1, 9999);
    assert!(!got.is_empty(), "clamped range still blames real lines");

    // A path that does not exist at that revision is simply a missing edge.
    assert!(
        b.blame_range("feat/b", "main", "does-not-exist.txt", 1, 1)
            .is_empty()
    );
    // As is a revision that does not resolve.
    assert!(
        b.blame_range("no-such-ref", "main", "f.txt", 1, 1)
            .is_empty()
    );
}

#[test]
fn hunk_parsing_matches_git_for_a_real_diff() {
    let r = edited_repo();
    let git = Git::new(&r.dir);
    let diff = diff_unified0(&git, "main", "feat/b").unwrap();
    let hunks = parse_old_side_hunks(&diff);

    // Lines 2 and 3 both changed and are adjacent, so git reports them as a single
    // old-side hunk of two lines rather than two one-line hunks.
    assert_eq!(hunks.keys().collect::<Vec<_>>(), vec!["f.txt"]);
    assert_eq!(hunks["f.txt"], vec![(2, 2)]);
}

#[test]
fn hunk_parsing_reports_additions_with_zero_old_lines() {
    let r = TestRepo::new();
    r.branch_from("feat/add", "main");
    r.commit_file("brand-new.txt", "one\ntwo\n", "feat: add a file");
    let git = Git::new(&r.dir);

    let diff = diff_unified0(&git, "main", "feat/add").unwrap();
    let hunks = parse_old_side_hunks(&diff);
    // A pure addition has no old-side lines: start 0, count 0.
    assert_eq!(hunks["brand-new.txt"], vec![(0, 0)]);
}

/// Drive the real pipeline shape: diff a branch against its upstream, then blame each
/// reported old-side range *in* that upstream, and report the commits found.
fn blame_edits_against_upstream(
    r: &TestRepo,
    upstream: &str,
    branch: &str,
) -> std::collections::HashSet<Sha> {
    let git = Git::new(&r.dir);
    let b = blamer(r);
    let diff = diff_unified0(&git, upstream, branch).unwrap();
    let mut blamed = std::collections::HashSet::new();
    for (path, ranges) in &parse_old_side_hunks(&diff) {
        for &(start, count) in ranges {
            let (lo, hi) = if count == 0 {
                (start, start + 1)
            } else {
                (start, start + count - 1)
            };
            blamed.extend(b.blame_range(upstream, "main", path, lo, hi));
        }
    }
    blamed
}

#[test]
fn editing_a_line_the_upstream_introduced_is_a_dependency() {
    // feat/dep rewrites line 2 - the very line feat/a introduced - so it genuinely
    // depends on feat/a.
    let r = edited_repo();
    r.checkout("feat/a");
    r.branch_from("feat/dep", "feat/a");
    r.commit_file("f.txt", "l1\nD2\nl3\n", "feat: rewrite line 2 again");
    r.checkout("main");

    let repo = RepoView::discover(&r.dir).unwrap();
    let a_commit = repo.rev_parse("feat/a").unwrap();
    let blamed = blame_edits_against_upstream(&r, "feat/a", "feat/dep");
    assert!(
        blamed.contains(&a_commit),
        "expected feat/a's commit among {blamed:?}"
    );
}

#[test]
fn editing_a_line_the_base_owns_is_not_a_dependency() {
    // feat/b rewrites line 3, which came from the base, not from feat/a. Blame bounded
    // to main..feat/a attributes it to a boundary commit, which is filtered out - so
    // no edge, and the branches are correctly siblings rather than a stack.
    let r = edited_repo();
    let blamed = blame_edits_against_upstream(&r, "feat/a", "feat/b");
    assert!(
        blamed.is_empty(),
        "base-owned line must not produce an edge, got {blamed:?}"
    );
}

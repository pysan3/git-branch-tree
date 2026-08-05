//! The content dependency engine against real repositories.
//!
//! Every case here is one the ancestry-only view gets wrong: branches that look
//! stacked but are independent, and branches that look independent but are not.

mod common;

use std::collections::BTreeMap;

use common::{Harness, TestRepo, analyse, expect_parents, parent_map};

fn deps(r: &TestRepo, branches: &[&str]) -> BTreeMap<String, Vec<String>> {
    parent_map(&analyse(r, branches))
}

#[test]
fn a_real_chain_keeps_only_nearest_edges() {
    // Each branch rewrites the line the previous one introduced, so this is a genuine
    // stack. Transitive reduction must leave a -> b -> c, not also a -> c.
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a rewrites line 1");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: b rewrites line 1");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("f.txt", "C1\nl2\nl3\n", "feat: c rewrites line 1");
    r.checkout("main");

    assert_eq!(
        deps(&r, &["feat/a", "feat/b", "feat/c"]),
        expect_parents(&[
            ("feat/a", &[]),
            ("feat/b", &["feat/a"]),
            ("feat/c", &["feat/b"]),
        ])
    );
}

#[test]
fn branches_stacked_in_git_but_touching_nothing_shared_are_independent() {
    // The headline case: b was branched off a, so ancestry says b depends on a - but
    // they touch disjoint files, so the real answer is that both sit on the base.
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b\n", "feat: b");
    r.checkout("main");

    assert_eq!(
        deps(&r, &["feat/a", "feat/b"]),
        expect_parents(&[("feat/a", &[]), ("feat/b", &[])])
    );
}

#[test]
fn editing_a_base_owned_line_is_not_a_dependency() {
    // Both branches touch the same file, but b edits a line that came from the base,
    // not from a. Same file is not the same as same code.
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a rewrites line 1");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "A1\nl2\nB3\n", "feat: b rewrites line 3");
    r.checkout("main");

    assert_eq!(
        deps(&r, &["feat/a", "feat/b"]),
        expect_parents(&[("feat/a", &[]), ("feat/b", &[])])
    );
}

#[test]
fn carrying_a_second_branch_new_file_without_ancestry_is_a_dependency() {
    // Content containment: feat/carrier cherry-picked feat/seed's new file, so it is
    // not a git descendant, yet it cannot land before feat/seed does.
    let r = TestRepo::new();
    r.branch_from("feat/seed", "main");
    r.commit_file("shared.txt", "shared content\n", "feat: add shared file");
    r.checkout("main");
    r.branch_from("feat/carrier", "main");
    // Carrier's own commit lands first, so the cherry-pick has a different parent and
    // is genuinely a new commit (onto the same parent it would be byte-identical).
    r.commit_file("extra.txt", "extra\n", "feat: carrier's own work");
    r.git(&["cherry-pick", "feat/seed"]);
    r.checkout("main");

    let repo = &Harness::new(&r).repo;
    let seed = repo.rev_parse("feat/seed").unwrap();
    let carrier = repo.rev_parse("feat/carrier").unwrap();
    assert!(
        !repo.is_ancestor(seed, carrier),
        "fixture must not be a git ancestor chain"
    );

    assert_eq!(
        deps(&r, &["feat/seed", "feat/carrier"]),
        expect_parents(&[("feat/carrier", &["feat/seed"]), ("feat/seed", &[])])
    );
}

#[test]
fn a_modified_base_file_carried_along_is_not_a_containment_dependency() {
    // Only *new* files count for containment: a file that already exists in the base
    // gets dropped by the rebase, so merely carrying a modification to it is not a
    // dependency (edits to it are caught by blame instead).
    let r = TestRepo::new();
    r.commit_file("existing.txt", "l1\nl2\nl3\nl4\n", "chore: seed");
    r.branch_from("feat/one", "main");
    r.commit_file("existing.txt", "X1\nl2\nl3\nl4\n", "feat: one edits line 1");
    r.checkout("main");
    r.branch_from("feat/two", "main");
    r.commit_file("existing.txt", "l1\nl2\nl3\nY4\n", "feat: two edits line 4");
    r.checkout("main");

    assert_eq!(
        deps(&r, &["feat/one", "feat/two"]),
        expect_parents(&[("feat/one", &[]), ("feat/two", &[])])
    );
}

#[test]
fn a_branch_editing_two_branches_lines_depends_on_both() {
    // A genuine diamond. Each branch owns one line of the shared file:
    //   root owns line 2, left owns line 1, right owns line 3.
    // feat/top then rewrites lines 1 and 3, so it depends on left and right - and on
    // root only transitively, which the reduction drops.
    let r = TestRepo::new();
    r.branch_from("feat/root", "main");
    r.commit_file("f.txt", "r1\nr2\nr3\n", "feat: root adds the file");
    r.branch_from("feat/left", "feat/root");
    r.commit_file("f.txt", "L1\nr2\nr3\n", "feat: left rewrites line 1");
    r.branch_from("feat/right", "feat/left");
    r.commit_file("f.txt", "L1\nr2\nR3\n", "feat: right rewrites line 3");
    r.branch_from("feat/top", "feat/right");
    r.commit_file("f.txt", "T1\nr2\nT3\n", "feat: top rewrites lines 1 and 3");
    r.checkout("main");

    let got = deps(&r, &["feat/root", "feat/left", "feat/right", "feat/top"]);
    assert_eq!(got["feat/root"], Vec::<String>::new());
    assert_eq!(got["feat/left"], vec!["feat/root"]);
    // right edits line 3, which root introduced - not left's line, so it is a sibling
    // of left despite being branched off it.
    assert_eq!(got["feat/right"], vec!["feat/root"]);
    assert_eq!(
        got["feat/top"],
        vec!["feat/left", "feat/right"],
        "top edits a line owned by each, so both edges are nearest"
    );
}

#[test]
fn excluded_paths_do_not_create_edges() {
    // A lockfile both branches churn must not tie them together.
    let r = TestRepo::new();
    r.commit_file("yarn.lock", "lock v1\nentry\n", "chore: seed lock");
    r.branch_from("feat/a", "main");
    r.commit_file("yarn.lock", "lock v2\nentry\n", "chore: a bumps lock");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("yarn.lock", "lock v3\nentry\n", "chore: b bumps lock");
    r.checkout("main");

    assert_eq!(
        deps(&r, &["feat/a", "feat/b"]),
        expect_parents(&[("feat/a", &[]), ("feat/b", &[])]),
        "default excludes must keep lockfile churn out of the graph"
    );
}

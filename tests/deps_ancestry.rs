//! `--ancestry` mode: trust the git graph instead of the content heuristics.

mod common;

use std::collections::BTreeMap;

use common::{TestRepo, analyse_by_ancestry, parent_map};

fn ancestry_deps(r: &TestRepo, branches: &[&str]) -> BTreeMap<String, Vec<String>> {
    parent_map(&analyse_by_ancestry(r, branches))
}

#[test]
fn a_git_stack_of_disjoint_branches_is_a_chain_under_ancestry() {
    // Deliberately the same fixture the content engine reports as independent: these
    // branches touch nothing in common, but each was branched off the previous one.
    // --ancestry is for when you *want* that literal reading.
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b\n", "feat: b");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("c.txt", "c\n", "feat: c");
    r.checkout("main");

    let got = ancestry_deps(&r, &["feat/a", "feat/b", "feat/c"]);
    assert_eq!(got["feat/a"], Vec::<String>::new());
    assert_eq!(got["feat/b"], vec!["feat/a"]);
    // Reduced to the nearest edge: a is an ancestor of c too, but only via b.
    assert_eq!(got["feat/c"], vec!["feat/b"]);
}

#[test]
fn unrelated_branches_have_no_ancestry_edges() {
    let r = TestRepo::new();
    r.branch_from("feat/x", "main");
    r.commit_file("x.txt", "x\n", "feat: x");
    r.checkout("main");
    r.branch_from("feat/y", "main");
    r.commit_file("y.txt", "y\n", "feat: y");
    r.checkout("main");

    let got = ancestry_deps(&r, &["feat/x", "feat/y"]);
    assert_eq!(got["feat/x"], Vec::<String>::new());
    assert_eq!(got["feat/y"], Vec::<String>::new());
}

#[test]
fn equal_tips_do_not_form_a_two_cycle() {
    // Two names for the same commit are mutually ancestors, which would be a cycle.
    // Rank breaks the tie so exactly one direction survives.
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.git(&["branch", "feat/alias", "feat/a"]);
    r.checkout("main");

    let got = ancestry_deps(&r, &["feat/a", "feat/alias"]);
    let edges = got["feat/a"].len() + got["feat/alias"].len();
    assert_eq!(edges, 1, "exactly one direction, not a 2-cycle: {got:?}");
    // Rank is (pidset size, name), so the alphabetically earlier name wins as parent.
    assert_eq!(got["feat/alias"], vec!["feat/a"]);
    assert_eq!(got["feat/a"], Vec::<String>::new());
}

#[test]
fn a_diamond_in_git_keeps_both_incoming_edges() {
    // A real merge commit: both sides are nearest ancestors of the merge.
    let r = TestRepo::new();
    r.branch_from("feat/root", "main");
    r.commit_file("root.txt", "root\n", "feat: root");
    r.branch_from("feat/left", "feat/root");
    r.commit_file("left.txt", "left\n", "feat: left");
    r.checkout("feat/root");
    r.branch_from("feat/right", "feat/root");
    r.commit_file("right.txt", "right\n", "feat: right");
    r.branch_from("feat/merge", "feat/right");
    r.git(&["merge", "-q", "--no-ff", "-m", "merge: left", "feat/left"]);
    r.checkout("main");

    let got = ancestry_deps(&r, &["feat/root", "feat/left", "feat/right", "feat/merge"]);
    assert_eq!(got["feat/root"], Vec::<String>::new());
    assert_eq!(got["feat/left"], vec!["feat/root"]);
    assert_eq!(got["feat/right"], vec!["feat/root"]);
    assert_eq!(got["feat/merge"], vec!["feat/left", "feat/right"]);
}

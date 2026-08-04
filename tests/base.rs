mod common;

use common::TestRepo;
use git_branch_tree::base::{detect_base, update_base, worktree_for_branch};
use git_branch_tree::gitx::{Git, RepoView};

fn views(r: &TestRepo) -> (RepoView, Git) {
    (RepoView::discover(&r.dir).unwrap(), Git::new(&r.dir))
}

#[test]
fn explicit_base_is_honoured_and_validated() {
    let r = TestRepo::new();
    let (repo, git) = views(&r);
    assert_eq!(detect_base(Some("main"), &repo, &git).unwrap(), "main");

    let err = detect_base(Some("nope"), &repo, &git).unwrap_err();
    assert_eq!(err.to_string(), "base ref 'nope' does not exist");
}

#[test]
fn falls_back_to_main_without_a_remote() {
    let r = TestRepo::new();
    let (repo, git) = views(&r);
    assert_eq!(detect_base(None, &repo, &git).unwrap(), "main");
}

#[test]
fn prefers_origin_head_symref() {
    let r = TestRepo::new();
    r.add_bare_origin();
    r.git(&["remote", "set-head", "origin", "-a"]);
    let (repo, git) = views(&r);
    // origin/HEAD -> origin/main, and a local `main` exists, so the local branch wins
    // (it is what gets rebased onto).
    assert_eq!(detect_base(None, &repo, &git).unwrap(), "main");
}

#[test]
fn resolves_to_remote_ref_when_no_local_branch_exists() {
    let r = TestRepo::new();
    r.add_bare_origin();
    r.git(&["remote", "set-head", "origin", "-a"]);
    // Leave `main` behind only as a remote-tracking ref.
    r.git(&["checkout", "-q", "-b", "work"]);
    r.git(&["branch", "-q", "-D", "main"]);
    let (repo, git) = views(&r);
    assert_eq!(detect_base(None, &repo, &git).unwrap(), "origin/main");
}

#[test]
fn worktree_for_branch_finds_the_checkout() {
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.checkout("main");
    let git = Git::new(&r.dir);

    // The main worktree has `main` checked out.
    let wt = worktree_for_branch(&git, "main").expect("main is checked out");
    assert_eq!(
        wt.canonicalize().unwrap(),
        r.dir.canonicalize().unwrap(),
        "main worktree path"
    );
    // `feat/a` is not checked out anywhere.
    assert!(worktree_for_branch(&git, "feat/a").is_none());

    // ... until it is, in a linked worktree.
    let linked = r.dir.parent().unwrap().join("wt-a");
    r.git(&["worktree", "add", "-q", linked.to_str().unwrap(), "feat/a"]);
    let found = worktree_for_branch(&git, "feat/a").expect("feat/a checked out in worktree");
    assert_eq!(
        found.canonicalize().unwrap(),
        linked.canonicalize().unwrap()
    );
}

#[test]
fn update_base_fast_forwards_a_checked_out_base() {
    let r = TestRepo::new();
    let bare = r.add_bare_origin();

    // Land a new commit on origin/main from a second clone.
    let other = r.dir.parent().unwrap().join("other");
    r.git_in(
        r.dir.parent().unwrap(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    r.git_in(&other, &["config", "user.name", "Other"]);
    r.git_in(&other, &["config", "user.email", "other@example.com"]);
    std::fs::write(other.join("upstream.txt"), "landed\n").unwrap();
    r.git_in(&other, &["add", "upstream.txt"]);
    r.git_in(&other, &["commit", "-q", "-m", "feat: landed upstream"]);
    r.git_in(&other, &["push", "-q", "origin", "main"]);
    let landed = r.git_in(&other, &["rev-parse", "HEAD"]);

    // `main` is checked out in r's main worktree, so a plain fetch could not move it;
    // update_base must pull inside that worktree instead.
    let before = r.sha("main");
    assert_ne!(before, landed);
    update_base(&Git::new(&r.dir), "main");
    assert_eq!(r.sha("main"), landed, "local main fast-forwarded");
}

#[test]
fn update_base_warns_and_continues_when_origin_is_unreachable() {
    let r = TestRepo::new();
    r.git(&["remote", "add", "origin", "/nonexistent/repo.git"]);
    let before = r.sha("main");
    // Best-effort: a failed refresh must not abort the run.
    update_base(&Git::new(&r.dir), "main");
    assert_eq!(r.sha("main"), before);
}

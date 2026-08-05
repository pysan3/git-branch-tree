use crate::gitx::{Git, RepoView};
use crate::testfix::TestRepo;

fn stacked_repo() -> TestRepo {
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a1\n", "feat: a1");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b1\n", "feat: b1");
    r.checkout("main");
    r
}

#[test]
fn subprocess_facade_runs_and_reports_errors() {
    let r = stacked_repo();
    let git = Git::new(&r.dir);
    assert_eq!(
        git.run(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
        "main"
    );
    assert!(git.ok(&["rev-parse", "--verify", "feat/a"]));
    assert!(!git.ok(&["rev-parse", "--verify", "refs/heads/nope"]));
    let err = git
        .run(&["rev-parse", "--verify", "refs/heads/nope"])
        .unwrap_err();
    assert_eq!(err.status, 128);
    assert!(
        err.to_string()
            .starts_with("git rev-parse --verify refs/heads/nope failed (128)")
    );
}

#[test]
fn repo_view_reads_refs_and_history() {
    let r = stacked_repo();
    let repo = RepoView::discover(&r.dir).unwrap();

    let branches = repo.local_branches().unwrap();
    assert_eq!(branches, vec!["feat/a", "feat/b", "main"]);
    assert!(repo.branch_exists("feat/a"));
    assert!(!repo.branch_exists("nope"));

    let main = repo.rev_parse("main").unwrap();
    let a = repo.rev_parse("feat/a").unwrap();
    let b = repo.rev_parse("feat/b").unwrap();
    assert_eq!(main.to_string(), r.sha("main"));

    // rev-list main..feat/b = [b1, a1] newest-first
    let shas = repo.rev_list(main, b).unwrap();
    assert_eq!(shas.len(), 2);
    assert_eq!(shas[0], b);
    assert_eq!(shas[1], a);

    assert_eq!(repo.merge_base(a, b), Some(a));
    assert!(repo.is_ancestor(main, b));
    assert!(repo.is_ancestor(a, b));
    assert!(!repo.is_ancestor(b, a));

    assert!(repo.path_in_tree(b, "a.txt"));
    assert!(!repo.path_in_tree(main, "a.txt"));
}

#[test]
fn raw_diff_lists_paths_and_blobs_without_renames() {
    let r = TestRepo::new();
    r.branch_from("feat/x", "main");
    r.commit_file("new.txt", "hello\n", "feat: add new");
    r.commit_file("README.md", "changed\n", "feat: edit readme");
    r.git(&["rm", "-q", "new.txt"]);
    r.git(&["commit", "-q", "-m", "feat: drop new"]);
    r.commit_file("moved.txt", "seed\n", "feat: copy of readme original");

    let repo = RepoView::discover(&r.dir).unwrap();
    let main = repo.rev_parse("main").unwrap();
    let x = repo.rev_parse("feat/x").unwrap();
    let mut changes = repo.raw_diff(main, x).unwrap();
    changes.sort_by(|l, r| l.path.cmp(&r.path));

    let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, vec!["README.md", "moved.txt"]);
    assert!(changes.iter().all(|c| c.blob.is_some()));

    // A deletion shows up with blob = None (diff across the commit that drops new.txt).
    let before = repo.rev_parse("feat/x~2").unwrap();
    let after = repo.rev_parse("feat/x~1").unwrap();
    let del = repo.raw_diff(before, after).unwrap();
    assert_eq!(del.len(), 1);
    assert_eq!(del[0].path, "new.txt");
    assert_eq!(del[0].blob, None);
}

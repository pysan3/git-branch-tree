//! Branch model construction and input-mode resolution against real repositories.

mod common;

use common::TestRepo;
use git_branch_tree::gitx::{Git, RepoView};
use git_branch_tree::input::resolve_branches;
use git_branch_tree::model::{BranchSet, build_branches};
use git_branch_tree::patchid::{PatchIdCache, patch_id_backend};

struct Ctx {
    repo: RepoView,
    cache: PatchIdCache,
    pool: rayon::ThreadPool,
}

fn ctx(r: &TestRepo) -> Ctx {
    let git = Git::new(&r.dir);
    Ctx {
        repo: RepoView::discover(&r.dir).unwrap(),
        cache: PatchIdCache::new(patch_id_backend(&r.dir, &git)),
        pool: rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap(),
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Classic linear stack: a <- b <- c, each adding one commit.
fn linear_stack() -> TestRepo {
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b\n", "feat: b");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("c.txt", "c\n", "feat: c");
    r.checkout("main");
    r
}

fn by_name<'a>(set: &'a BranchSet, name: &str) -> &'a git_branch_tree::model::Branch {
    set.get(set.by_name(name).expect("branch present"))
}

#[test]
fn build_branches_finds_chain_upstream_and_own_commits() {
    let r = linear_stack();
    let c = ctx(&r);
    let base = c.repo.rev_parse("main").unwrap();
    let set = build_branches(
        &names(&["feat/a", "feat/b", "feat/c"]),
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();

    // Each branch carries its ancestors' content plus one commit of its own.
    assert_eq!(by_name(&set, "feat/a").pidset.len(), 1);
    assert_eq!(by_name(&set, "feat/b").pidset.len(), 2);
    assert_eq!(by_name(&set, "feat/c").pidset.len(), 3);

    // prev = largest strict subset, i.e. the nearest upstream in the chain.
    assert_eq!(by_name(&set, "feat/a").prev, None);
    assert_eq!(
        by_name(&set, "feat/b").prev,
        set.by_name("feat/a"),
        "b's nearest upstream is a"
    );
    assert_eq!(
        by_name(&set, "feat/c").prev,
        set.by_name("feat/b"),
        "c's nearest upstream is b"
    );

    // own_shas is what each branch adds beyond its upstream.
    for n in ["feat/a", "feat/b", "feat/c"] {
        assert_eq!(by_name(&set, n).own_shas.len(), 1, "{n} owns one commit");
    }

    // rank orders upstream-first, so ids_by_rank is the chain order.
    let ranked: Vec<&str> = set
        .ids_by_rank()
        .into_iter()
        .map(|b| set.get(b).name.as_str())
        .collect();
    assert_eq!(ranked, vec!["feat/a", "feat/b", "feat/c"]);
}

#[test]
fn independent_siblings_have_no_upstream() {
    let r = TestRepo::new();
    r.branch_from("feat/x", "main");
    r.commit_file("x.txt", "x\n", "feat: x");
    r.checkout("main");
    r.branch_from("feat/y", "main");
    r.commit_file("y.txt", "y\n", "feat: y");
    r.checkout("main");

    let c = ctx(&r);
    let base = c.repo.rev_parse("main").unwrap();
    let set = build_branches(
        &names(&["feat/x", "feat/y"]),
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();

    assert_eq!(by_name(&set, "feat/x").prev, None);
    assert_eq!(by_name(&set, "feat/y").prev, None);
    assert_eq!(by_name(&set, "feat/x").own_shas.len(), 1);
    assert_eq!(by_name(&set, "feat/y").own_shas.len(), 1);
}

#[test]
fn single_branch_discovers_everything_stacked_on_it_by_content() {
    let r = linear_stack();
    // A branch that does NOT carry feat/a's change must stay out of the result.
    r.branch_from("feat/unrelated", "main");
    r.commit_file("u.txt", "u\n", "feat: u");
    r.checkout("main");

    let c = ctx(&r);
    let base = c.repo.rev_parse("main").unwrap();
    let got = resolve_branches(
        &names(&["feat/a"]),
        &[],
        false,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();
    assert_eq!(got, vec!["feat/a", "feat/b", "feat/c"]);
}

#[test]
fn stacked_on_discovery_survives_a_rebase() {
    // The point of using patch-ids: rewriting the shas must not lose the stack.
    let r = linear_stack();
    // Move main forward, then rebase the stack onto it, giving every commit a new sha.
    r.checkout("main");
    r.commit_file("unrelated.txt", "m\n", "chore: move main");
    for b in ["feat/a", "feat/b", "feat/c"] {
        r.checkout(b);
        r.git(&["rebase", "-q", "main"]);
    }
    r.checkout("main");

    let c = ctx(&r);
    let base = c.repo.rev_parse("main").unwrap();
    let got = resolve_branches(
        &names(&["feat/a"]),
        &[],
        false,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();
    assert_eq!(got, vec!["feat/a", "feat/b", "feat/c"]);
}

#[test]
fn prefix_mode_selects_matching_local_branches() {
    let r = TestRepo::new();
    for b in ["PROJ-412/one", "PROJ-500/two", "OPS-7/three"] {
        r.checkout("main");
        r.branch_from(b, "main");
        r.commit_file(&format!("{}.txt", b.replace('/', "_")), "x\n", "feat: x");
    }
    r.checkout("main");

    let c = ctx(&r);
    let base = c.repo.rev_parse("main").unwrap();

    // Literal prefix.
    let got = resolve_branches(
        &[],
        &names(&["PROJ-412"]),
        false,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();
    assert_eq!(got, vec!["PROJ-412/one"]);

    // --alpha widens to the leading-letter group, so both PROJ tickets match.
    let got = resolve_branches(
        &[],
        &names(&["PROJ-412"]),
        true,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();
    assert_eq!(got, vec!["PROJ-412/one", "PROJ-500/two"]);

    // A prefix matching nothing is an error rather than an empty report.
    let err = resolve_branches(
        &[],
        &names(&["NOPE"]),
        false,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap_err();
    assert!(err.to_string().contains("no local branches match"));
}

#[test]
fn explicit_list_is_kept_in_order_and_deduped() {
    let r = linear_stack();
    let c = ctx(&r);
    let base = c.repo.rev_parse("main").unwrap();

    let got = resolve_branches(
        &names(&["feat/c", "feat/a", "feat/c"]),
        &[],
        false,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap();
    assert_eq!(got, vec!["feat/c", "feat/a"]);

    let err = resolve_branches(
        &names(&["feat/a", "ghost"]),
        &[],
        false,
        base,
        &c.repo,
        &c.cache,
        &c.pool,
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "branch 'ghost' does not exist");
}

//! The two patch-id backends must agree, because the whole dependency engine keys
//! commit identity on them: a disagreement would silently move dependency edges.
//! The subprocess pipeline is the golden reference (it *is* git); git2 is the fast
//! default that has to match it.

mod common;

use common::TestRepo;
use git_branch_tree::gitx::{Git, RepoView, Sha};
use git_branch_tree::patchid::{
    Git2PatchIds, PatchIdCache, PatchIdSource, SubprocessPatchIds, patch_id_backend,
};

/// A repo exercising every shape that has (or lacks) a patch-id.
fn mixed_repo() -> TestRepo {
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a1\n", "feat: a1");
    r.commit_file("a.txt", "a1\na2\n", "feat: a2");
    // An empty commit has no diff, so no patch-id.
    r.git(&["commit", "-q", "--allow-empty", "-m", "chore: empty"]);
    // A merge commit prints no diff under `diff-tree -p`, so no patch-id either.
    r.branch_from("feat/b", "main");
    r.commit_file("b.txt", "b1\n", "feat: b1");
    r.git(&["merge", "-q", "--no-ff", "-m", "merge: a into b", "feat/a"]);
    r
}

fn all_commits(r: &TestRepo) -> Vec<Sha> {
    r.git(&["rev-list", "--all"])
        .lines()
        .map(|l| l.parse::<Sha>().expect("parse sha"))
        .collect()
}

#[test]
fn git2_and_subprocess_backends_agree() {
    let r = mixed_repo();
    let shas = all_commits(&r);
    assert!(shas.len() >= 5, "fixture should cover several shapes");

    let subprocess = SubprocessPatchIds {
        git: Git::new(&r.dir),
    };
    let git2 = Git2PatchIds {
        repo_path: r.dir.clone(),
    };

    let want = subprocess.compute(&shas).unwrap();
    let got = git2.compute(&shas).unwrap();

    assert_eq!(want.len(), shas.len());
    for sha in &shas {
        assert_eq!(
            got.get(sha),
            want.get(sha),
            "backends disagree on {sha}:\n  git2={:?}\n  git ={:?}",
            got.get(sha),
            want.get(sha),
        );
    }
    // The fixture must genuinely exercise both outcomes, or this test proves nothing.
    assert!(
        want.values().any(|p| p.is_some()),
        "expected some commits to have a patch-id"
    );
    assert!(
        want.values().any(|p| p.is_none()),
        "expected empty/merge/root commits to have no patch-id"
    );
}

#[test]
fn identical_content_shares_a_patch_id_across_rewrites() {
    // The property the whole tool rests on: a cherry-picked (or squash-replayed)
    // change keeps its patch-id even though its commit sha differs.
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("shared.txt", "content\n", "feat: shared change");
    let original = r.sha("HEAD");

    r.checkout("main");
    r.branch_from("feat/copy", "main");
    // Land the copy on a different parent, so the commit object genuinely differs
    // (with an identical parent, tree, message and pinned dates it would be the very
    // same commit). This is also the realistic shape: the change replayed elsewhere.
    r.commit_file("other.txt", "unrelated\n", "feat: unrelated work");
    r.git(&["cherry-pick", "feat/a"]);
    let copied = r.sha("HEAD");
    assert_ne!(original, copied, "cherry-pick makes a new commit");

    for source in [
        Box::new(SubprocessPatchIds {
            git: Git::new(&r.dir),
        }) as Box<dyn PatchIdSource>,
        Box::new(Git2PatchIds {
            repo_path: r.dir.clone(),
        }),
    ] {
        let shas = [
            original.parse::<Sha>().unwrap(),
            copied.parse::<Sha>().unwrap(),
        ];
        let ids = source.compute(&shas).unwrap();
        assert_eq!(
            ids[&shas[0]], ids[&shas[1]],
            "cherry-pick must preserve the patch-id"
        );
        assert!(ids[&shas[0]].is_some());
    }
}

#[test]
fn cache_primes_once_and_serves_reads() {
    let r = mixed_repo();
    let shas = all_commits(&r);
    let cache = PatchIdCache::new(patch_id_backend(&r.dir, &Git::new(&r.dir)));

    // Nothing is known before priming.
    assert!(cache.get(shas[0]).is_none());
    cache.prime(&shas).unwrap();

    let direct = SubprocessPatchIds {
        git: Git::new(&r.dir),
    }
    .compute(&shas)
    .unwrap();
    for sha in &shas {
        assert_eq!(cache.get(*sha), direct[sha], "cached value for {sha}");
    }

    // Re-priming is a no-op and keeps answers stable.
    cache.prime(&shas).unwrap();
    for sha in &shas {
        assert_eq!(cache.get(*sha), direct[sha]);
    }
}

#[test]
fn patch_ids_ignore_commit_metadata() {
    // A patch-id hashes the diff, not the commit: message, author and date all differ
    // here. This is what survives a rebase, where every commit is re-authored.
    let r = TestRepo::new();
    for (branch, msg, who) in [
        ("feat/one", "feat: add both files", "One <one@example.com>"),
        (
            "feat/two",
            "chore: totally different subject",
            "Two <two@example.com>",
        ),
    ] {
        r.checkout("main");
        r.branch_from(branch, "main");
        std::fs::write(r.dir.join("x.txt"), "1\n").unwrap();
        std::fs::write(r.dir.join("y.txt"), "2\n").unwrap();
        r.git(&["add", "x.txt", "y.txt"]);
        r.git(&["commit", "-q", "--author", who, "-m", msg]);
    }
    let repo = RepoView::discover(&r.dir).unwrap();
    let one = repo.rev_parse("feat/one").unwrap();
    let two = repo.rev_parse("feat/two").unwrap();
    assert_ne!(one, two, "different commits");

    let ids = SubprocessPatchIds {
        git: Git::new(&r.dir),
    }
    .compute(&[one, two])
    .unwrap();
    assert_eq!(
        ids[&one], ids[&two],
        "same diff must yield the same patch-id regardless of commit metadata"
    );
    assert!(ids[&one].is_some());
}

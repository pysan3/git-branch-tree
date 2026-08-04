//! Renderer output, pinned exactly - it is the tool's entire user interface.

mod common;

use std::collections::HashSet;

use common::TestRepo;
use git_branch_tree::blame::SubprocessBlamer;
use git_branch_tree::deps::compute_dependencies;
use git_branch_tree::exclude::ExcludeSet;
use git_branch_tree::gitx::{Git, RepoView};
use git_branch_tree::model::{BranchSet, build_branches};
use git_branch_tree::patchid::{PatchIdCache, patch_id_backend};
use git_branch_tree::render::{render_ascii, render_header, render_mermaid};

/// Run the full analysis so the renderers are exercised on real graphs.
fn analysed(r: &TestRepo, branches: &[&str]) -> BranchSet {
    let git = Git::new(&r.dir);
    let repo = RepoView::discover(&r.dir).unwrap();
    let cache = PatchIdCache::new(patch_id_backend(&r.dir, &git));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let base = repo.rev_parse("main").unwrap();
    let names: Vec<String> = branches.iter().map(|s| s.to_string()).collect();
    let mut set = build_branches(&names, base, &repo, &cache, &pool).unwrap();
    let blamer = SubprocessBlamer { git: git.clone() };
    let exclude = ExcludeSet::new(&[], true).unwrap();
    compute_dependencies(
        &mut set, "main", base, &repo, &git, &blamer, &cache, &exclude, &pool,
    )
    .unwrap();
    set
}

fn merged(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// root owns line 2, left owns line 1, right owns line 3, top edits 1 and 3 - so top
/// depends on both left and right.
fn diamond_repo() -> TestRepo {
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
    r
}

#[test]
fn ascii_renders_a_chain_as_nested_branches() {
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: b");
    r.checkout("main");

    let set = analysed(&r, &["feat/a", "feat/b"]);
    assert_eq!(
        render_ascii(&set, "main", &HashSet::new()),
        "main\n└─ feat/a\n   └─ feat/b"
    );
}

#[test]
fn ascii_renders_independent_branches_side_by_side() {
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b\n", "feat: b");
    r.checkout("main");

    // Stacked in git, independent in content: both hang off the base.
    let set = analysed(&r, &["feat/a", "feat/b"]);
    assert_eq!(
        render_ascii(&set, "main", &HashSet::new()),
        "main\n├─ feat/a\n└─ feat/b"
    );
}

#[test]
fn ascii_annotates_a_second_parent_instead_of_duplicating_the_subtree() {
    let r = diamond_repo();
    let set = analysed(&r, &["feat/root", "feat/left", "feat/right", "feat/top"]);
    assert_eq!(
        render_ascii(&set, "main", &HashSet::new()),
        "\
main
└─ feat/root
   ├─ feat/left
   └─ feat/right
      └─ feat/top   (also depends on: feat/left)"
    );
}

#[test]
fn mermaid_expresses_multiple_parents_natively() {
    let r = diamond_repo();
    let set = analysed(&r, &["feat/root", "feat/left", "feat/right", "feat/top"]);
    assert_eq!(
        render_mermaid(&set, "main", &HashSet::new()),
        "\
```mermaid
graph TD
  b_main([\"main\"])
  b_feat_root[\"feat/root\"]
  b_feat_left[\"feat/left\"]
  b_feat_right[\"feat/right\"]
  b_feat_top[\"feat/top\"]
  b_main --> b_feat_root
  b_feat_root --> b_feat_left
  b_feat_root --> b_feat_right
  b_feat_left --> b_feat_top
  b_feat_right --> b_feat_top
```"
    );
}

#[test]
fn a_merged_branch_collapses_into_the_base() {
    let r = diamond_repo();
    let set = analysed(&r, &["feat/root", "feat/left", "feat/right", "feat/top"]);

    // root has landed: it *is* the base now, so it is omitted and the branches that
    // depended only on it hang directly off the base.
    assert_eq!(
        render_ascii(&set, "main", &merged(&["feat/root"])),
        "\
main
├─ feat/left
└─ feat/right
   └─ feat/top   (also depends on: feat/left)"
    );

    // Mermaid agrees: no node for root, and its former children point at the base.
    let m = render_mermaid(&set, "main", &merged(&["feat/root"]));
    assert!(
        !m.contains("b_feat_root"),
        "merged branch must be gone:\n{m}"
    );
    assert!(m.contains("  b_main --> b_feat_left"));
    assert!(m.contains("  b_main --> b_feat_right"));
}

#[test]
fn merging_an_inner_branch_repoints_its_dependants() {
    let r = diamond_repo();
    let set = analysed(&r, &["feat/root", "feat/left", "feat/right", "feat/top"]);

    // With left merged, top keeps only its right-hand dependency, so the annotation
    // disappears rather than naming a landed branch.
    assert_eq!(
        render_ascii(&set, "main", &merged(&["feat/root", "feat/left"])),
        "main\n└─ feat/right\n   └─ feat/top"
    );
}

#[test]
fn mermaid_sanitises_branch_names_into_node_ids() {
    let r = TestRepo::new();
    r.branch_from("PROJ-412/feat.thing", "main");
    r.commit_file("x.txt", "x\n", "feat: x");
    r.checkout("main");

    let set = analysed(&r, &["PROJ-412/feat.thing"]);
    let m = render_mermaid(&set, "main", &HashSet::new());
    // Punctuation becomes underscores in the id, while the label keeps the real name.
    assert!(
        m.contains("b_PROJ_412_feat_thing[\"PROJ-412/feat.thing\"]"),
        "{m}"
    );
    assert!(m.contains("b_main --> b_PROJ_412_feat_thing"), "{m}");
}

#[test]
fn header_lists_base_and_branches() {
    let r = diamond_repo();
    let set = analysed(&r, &["feat/root", "feat/left"]);
    assert_eq!(
        render_header(&set, "main", &[]),
        "# base: main\n# branches (2): feat/root, feat/left"
    );
    assert_eq!(
        render_header(&set, "main", &["feat/left".to_string()]),
        "# base: main\n# branches (2): feat/root, feat/left\n\
         # auto-detected as merged on GitHub: feat/left"
    );
}

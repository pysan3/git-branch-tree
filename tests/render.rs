//! Renderer output, pinned exactly - it is the tool's entire user interface.

mod common;

use std::collections::HashSet;

use common::{
    DIAMOND, TestRepo, analyse as analysed, diamond_under_root as diamond_repo, names as merged,
};
use git_branch_tree::plan::rebase_plan;
use git_branch_tree::render::{render_ascii, render_header, render_mermaid};

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
    let set = analysed(&r, DIAMOND);
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
    let set = analysed(&r, DIAMOND);
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
    let set = analysed(&r, DIAMOND);

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
    let set = analysed(&r, DIAMOND);

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
fn the_tree_and_the_plan_agree_on_where_each_branch_hangs() {
    // These were computed independently once - the tree took the highest-ranked open
    // parent one way, the planner another - so they could drift into drawing a branch
    // under one parent while rebasing it onto a different one. They now share a
    // definition; this pins that they cannot disagree, under every merge state.
    let r = diamond_repo();
    let set = analysed(&r, DIAMOND);

    for landed in [
        merged(&[]),
        merged(&["feat/root"]),
        merged(&["feat/root", "feat/left"]),
        merged(&["feat/left"]),
    ] {
        let plan = rebase_plan(&set, "main", &landed, &std::collections::BTreeSet::new());
        for entry in &plan {
            let drawn = set
                .primary_open_parent(entry.branch, &landed)
                .map_or_else(|| "main".to_string(), |p| set.get(p).name.clone());
            assert_eq!(
                entry.onto,
                drawn,
                "{} is drawn under {drawn} but rebases onto {} (merged: {landed:?})",
                set.get(entry.branch).name,
                entry.onto
            );
        }
    }
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

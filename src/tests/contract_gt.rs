//! Contract test: the *real* `gt` against the parser that reads it.
//!
//! Every other test in this crate feeds the parser output someone typed into a string
//! literal, which proves the parser handles what we believe gt prints. This proves what
//! gt actually prints - the only failure this feature has that our own code cannot
//! cause. Graphite ships a release, moves a glyph, and every stubbed test stays green
//! while `--from-gt-stack` quietly finds nothing.
//!
//! `#[ignore]`d rather than skipped when `gt` is missing: a test that silently passes on
//! a machine without the tool reports success while asserting nothing, which is worse
//! than not having it. Run it deliberately:
//!
//! ```sh
//! cargo test -- --ignored
//! ```
//!
//! Deliberately narrow. This is not a test of Graphite - only that the bytes it prints
//! are the bytes we parse. It needs no account and no network.

use crate::gitx::Git;
use crate::stacks::StackTool;
use crate::stacks::gt::GtStack;
use crate::testfix::TestRepo;

/// A throwaway `HOME` for the `gt` child processes.
///
/// gt keeps user config in `$XDG_CONFIG_HOME/graphite` and unpacks ~12 MB of bundled
/// native dependencies into `$HOME/.cache/pkg`. Both are redirected here so a test run
/// cannot read, write or slow down a real Graphite install on the machine - and so the
/// leftovers leave with the `TempDir` rather than accumulating in someone's home. CI
/// would not care; a maintainer running `cargo test -- --ignored` would.
struct GtHome {
    dir: tempfile::TempDir,
}

impl GtHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create temp home");
        let config = dir.path().join(".config/graphite");
        std::fs::create_dir_all(&config).expect("create graphite config dir");
        // A fresh config defaults to updateAutomatically, which would have gt reaching
        // for a new version in the middle of a test: network, and a moving target.
        std::fs::write(
            config.join("user_config"),
            r#"{"updateAutomatically": false}"#,
        )
        .expect("write graphite user config");
        Self { dir }
    }

    /// Run `gt` in the repo, panicking with its stderr on failure.
    fn gt(&self, r: &TestRepo, args: &[&str]) -> String {
        let spec = GtStack.spec();
        let home = self.dir.path().display().to_string();
        let config = self.dir.path().join(".config").display().to_string();
        let mut env: Vec<(&str, &str)> = spec.env.to_vec();
        env.push(("HOME", &home));
        env.push(("XDG_CONFIG_HOME", &config));
        Git::new(&r.dir)
            .tool(spec.program, args, &env)
            .unwrap_or_else(|e| panic!("gt {args:?} failed: {e:#}"))
    }

    /// Set up a tracked repository with `main` as its trunk.
    fn init(&self) -> TestRepo {
        let r = TestRepo::new();
        self.gt(&r, &["init", "--trunk", "main"]);
        r
    }

    /// `gt create` a branch off `parent`, with a file of its own.
    fn create(&self, r: &TestRepo, name: &str, parent: &str) {
        r.checkout(parent);
        let file = format!("{}.txt", name.replace('/', "_"));
        std::fs::write(r.dir.join(&file), format!("{name}\n")).expect("write file");
        r.git(&["add", "-A"]);
        self.gt(r, &["create", name, "-m", &format!("feat: {name}")]);
    }

    /// What the tool itself would list, parsed the way the tool parses it.
    fn listed(&self, r: &TestRepo) -> Vec<String> {
        GtStack.parse(&self.gt(r, GtStack.spec().list_args))
    }
}

#[test]
#[ignore = "needs a real `gt` on PATH; run with `cargo test -- --ignored`"]
fn the_parser_reads_what_graphite_actually_prints() {
    let home = GtHome::new();
    let r = home.init();

    // A fork, so the drawing has to collapse branches - the shape most likely to move
    // between gt releases, and the one a single linear chain would never exercise.
    for (name, parent) in [
        ("PROJ-1/api", "main"),
        ("PROJ-1/handler", "PROJ-1/api"),
        ("PROJ-1/ui", "PROJ-1/handler"),
        ("PROJ-1/second-child", "PROJ-1/api"),
    ] {
        home.create(&r, name, parent);
    }

    // From the fork root, so the whole subtree is in view - `--stack` is relative to the
    // checked-out branch, which `the_listing_follows_the_checked_out_branch` pins.
    r.checkout("PROJ-1/api");

    // Exactly the command and environment the tool itself issues, from the same Spec -
    // so a wrong `list_args` fails here too, not just a wrong parser.
    let out = home.gt(&r, GtStack.spec().list_args);
    let parsed = GtStack.parse(&out);

    for expected in [
        "PROJ-1/api",
        "PROJ-1/handler",
        "PROJ-1/ui",
        "PROJ-1/second-child",
        "main",
    ] {
        assert!(
            parsed.iter().any(|n| n == expected),
            "gt named {expected:?} but the parser did not find it.\n\
             parsed: {parsed:?}\nraw output:\n{out}"
        );
    }

    // Nothing invented: every name must be a branch that exists. This is what catches a
    // rendering change that leaves box-drawing behind as a plausible-looking token.
    let branches = r.git(&["for-each-ref", "--format=%(refname:short)", "refs/heads"]);
    let known: Vec<&str> = branches.lines().collect();
    for name in &parsed {
        assert!(
            known.contains(&name.as_str()),
            "the parser produced {name:?}, which is not a branch in the repository.\n\
             parsed: {parsed:?}\nraw output:\n{out}"
        );
    }
}

#[test]
#[ignore = "needs a real `gt` on PATH; run with `cargo test -- --ignored`"]
fn the_listing_follows_the_checked_out_branch() {
    // `--stack` means the current branch's ancestors and descendants, not every branch
    // in the tree it belongs to. From a leaf you therefore get your own line to the
    // trunk and nothing sideways - so which branch is checked out changes what
    // --from-gt-stack analyses. Surprising enough to pin, and only a real gt can say it.
    let home = GtHome::new();
    let r = home.init();

    for (name, parent) in [
        ("PROJ-1/api", "main"),
        ("PROJ-1/handler", "PROJ-1/api"),
        ("PROJ-1/sibling", "PROJ-1/api"),
    ] {
        home.create(&r, name, parent);
    }

    r.checkout("PROJ-1/api");
    let from_root = home.listed(&r);
    assert!(
        from_root.iter().any(|n| n == "PROJ-1/handler")
            && from_root.iter().any(|n| n == "PROJ-1/sibling"),
        "the fork root should see both children: {from_root:?}"
    );

    r.checkout("PROJ-1/handler");
    let from_leaf = home.listed(&r);
    assert!(
        from_leaf.iter().any(|n| n == "PROJ-1/api"),
        "a leaf should still see its ancestors: {from_leaf:?}"
    );
    assert!(
        !from_leaf.iter().any(|n| n == "PROJ-1/sibling"),
        "a leaf saw a sibling, so --stack is no longer relative to HEAD: {from_leaf:?}"
    );
}

#[test]
#[ignore = "needs a real `gt` on PATH; run with `cargo test -- --ignored`"]
fn the_listing_is_scoped_to_one_stack() {
    // `gt log short` without --stack lists every tracked branch in the repository, so
    // dropping the flag would pull unrelated work into the analysis. That is a property
    // of gt rather than of our code, so only a real gt can hold it.
    let home = GtHome::new();
    let r = home.init();

    for (name, parent) in [("PROJ-1/api", "main"), ("OPS-9/unrelated", "main")] {
        home.create(&r, name, parent);
    }

    r.checkout("PROJ-1/api");
    let scoped = home.listed(&r);

    assert!(
        scoped.iter().any(|n| n == "PROJ-1/api"),
        "scoped listing lost its own branch: {scoped:?}"
    );
    assert!(
        !scoped.iter().any(|n| n == "OPS-9/unrelated"),
        "scoped listing leaked an unrelated stack, so --stack no longer scopes: {scoped:?}"
    );
}

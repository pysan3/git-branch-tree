//! Byte-for-byte comparison against the Python implementation this crate replaced.
//!
//! Ignored by default and gated on `GBT_PYTHON`, because it needs that script on the
//! machine. Run it with:
//!
//! ```text
//! GBT_PYTHON=~/dotfiles/bin/git-branch-tree cargo test --test golden_python -- --ignored
//! ```
//!
//! The point is not that the Rust port is *similar*: the emitted rebase block gets
//! pasted into a shell, so any drift in it is a behaviour change. Every fixture is run
//! through both implementations under the same flags and the stdout must match exactly.

mod common;

use std::path::Path;
use std::process::Command;

use common::TestRepo;

/// The Python implementation, or `None` when the harness is not configured.
fn python() -> Option<String> {
    let raw = std::env::var("GBT_PYTHON").ok()?;
    let expanded = match raw.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()),
        None => raw,
    };
    assert!(
        Path::new(&expanded).is_file(),
        "GBT_PYTHON does not point at a file: {expanded}"
    );
    Some(expanded)
}

fn run_python(script: &str, dir: &Path, args: &[&str]) -> String {
    let out = Command::new("python3")
        .arg(script)
        .arg("--no-fetch")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run the python implementation");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_rust(dir: &Path, args: &[&str]) -> String {
    let exe = assert_cmd::cargo::cargo_bin("git-branch-tree");
    let out = Command::new(exe)
        .arg("--no-fetch")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run the rust implementation");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Compare both implementations across every flag combination, on one fixture.
fn compare_all(label: &str, r: &TestRepo, branches: &[&str]) {
    let Some(script) = python() else {
        eprintln!("GBT_PYTHON not set; skipping");
        return;
    };

    // Flag sets that change the report rather than just the plumbing.
    let variants: Vec<Vec<&str>> = vec![
        vec!["--format", "both"],
        vec!["--format", "ascii"],
        vec!["--format", "mermaid"],
        vec!["--format", "both", "--ancestry"],
        vec!["--format", "both", "--skip-ambiguous"],
        vec!["--format", "both", "--no-rebase"],
        vec!["--format", "both", "--no-default-exclude"],
        vec!["--format", "ascii", "-j", "1"],
    ];

    for variant in &variants {
        let mut args: Vec<&str> = variant.clone();
        args.extend_from_slice(branches);
        let py = run_python(&script, &r.dir, &args);
        let rs = run_rust(&r.dir, &args);
        assert_eq!(
            py, rs,
            "output differs for {label} with {variant:?}\n--- python ---\n{py}\n--- rust ---\n{rs}"
        );
        assert!(!py.is_empty(), "{label}: fixture produced no report at all");
    }
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn linear_stack() {
    // Each branch rewrites the line the previous one introduced: a genuine chain.
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: b");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("f.txt", "C1\nl2\nl3\n", "feat: c");
    r.checkout("main");
    compare_all("linear stack", &r, &["feat/a", "feat/b", "feat/c"]);
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn independent_siblings() {
    // Stacked in git, independent in content - the case the tool exists to flatten.
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("b.txt", "b\n", "feat: b");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("c.txt", "c\n", "feat: c");
    r.checkout("main");
    compare_all("independent siblings", &r, &["feat/a", "feat/b", "feat/c"]);
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn diamond_with_two_parents() {
    let r = TestRepo::new();
    r.commit_file("f.txt", "r1\nr2\nr3\n", "chore: seed");
    r.branch_from("feat/root", "main");
    r.commit_file("f.txt", "r1\nR2\nr3\n", "feat: root");
    r.branch_from("feat/left", "feat/root");
    r.commit_file("f.txt", "L1\nR2\nr3\n", "feat: left");
    r.branch_from("feat/right", "feat/left");
    r.commit_file("f.txt", "L1\nR2\nG3\n", "feat: right");
    r.branch_from("feat/top", "feat/right");
    r.commit_file("f.txt", "T1\nR2\nT3\n", "feat: top");
    r.checkout("main");
    compare_all(
        "diamond",
        &r,
        &["feat/root", "feat/left", "feat/right", "feat/top"],
    );
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn squash_merged_branch_gives_a_skip_point() {
    // The squash lands the content under a brand-new hash, so the skip point has to be
    // found by patch-id rather than by ancestry.
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/landed", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: landed");
    r.branch_from("feat/next", "feat/landed");
    r.commit_file("f.txt", "A1\nB2\nl3\n", "feat: next");
    r.squash_merge_into_main("feat/landed");

    let Some(_) = python() else { return };
    let branches = ["feat/landed", "feat/next"];
    let script = python().unwrap();
    for variant in [
        vec!["--format", "both", "--merged", "feat/landed"],
        vec!["--format", "ascii", "--merged", "feat/landed"],
    ] {
        // `--merged` takes one-or-more values, so the branches must precede it.
        let mut args: Vec<&str> = vec![variant[0], variant[1]];
        args.extend_from_slice(&branches);
        args.extend_from_slice(&variant[2..]);
        let py = run_python(&script, &r.dir, &args);
        let rs = run_rust(&r.dir, &args);
        assert_eq!(py, rs, "squash-merge output differs with {args:?}");
    }
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn content_containment_without_ancestry() {
    let r = TestRepo::new();
    r.branch_from("feat/seed", "main");
    r.commit_file("shared.txt", "shared\n", "feat: shared");
    r.checkout("main");
    r.branch_from("feat/carrier", "main");
    r.commit_file("extra.txt", "extra\n", "feat: own work");
    r.git(&["cherry-pick", "feat/seed"]);
    r.checkout("main");
    compare_all("containment", &r, &["feat/seed", "feat/carrier"]);
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn excluded_lockfiles() {
    let r = TestRepo::new();
    r.commit_file("yarn.lock", "v1\nentry\n", "chore: seed lock");
    r.branch_from("feat/a", "main");
    r.commit_file("yarn.lock", "v2\nentry\n", "chore: a bumps lock");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("yarn.lock", "v3\nentry\n", "chore: b bumps lock");
    r.checkout("main");
    compare_all("lockfiles", &r, &["feat/a", "feat/b"]);
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn prefix_and_alpha_modes() {
    let Some(script) = python() else { return };
    let r = TestRepo::new();
    for b in ["PROJ-1/one", "PROJ-2/two", "OTHER-9/three"] {
        r.checkout("main");
        r.branch_from(b, "main");
        r.commit_file(&format!("{}.txt", b.replace('/', "_")), "x\n", "feat: x");
    }
    r.checkout("main");

    for args in [
        vec!["--format", "both", "--prefix", "PROJ-1"],
        vec!["--format", "both", "--prefix", "PROJ-1", "PROJ-2"],
        vec!["--format", "both", "--alpha", "--prefix", "PROJ-1"],
    ] {
        let py = run_python(&script, &r.dir, &args);
        let rs = run_rust(&r.dir, &args);
        assert_eq!(py, rs, "prefix output differs with {args:?}");
    }
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn single_branch_stack_discovery() {
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: b");
    r.checkout("main");
    r.branch_from("feat/unrelated", "main");
    r.commit_file("u.txt", "u\n", "feat: u");
    r.checkout("main");
    compare_all("single branch discovery", &r, &["feat/a"]);
}

#[test]
#[ignore = "needs GBT_PYTHON pointing at the python implementation"]
fn error_messages_and_exit_codes_match() {
    let Some(script) = python() else { return };
    let r = TestRepo::new();
    r.branch_from("feat/a", "main");
    r.commit_file("a.txt", "a\n", "feat: a");
    r.checkout("main");

    for args in [vec!["ghost"], vec![]] {
        let py = Command::new("python3")
            .arg(&script)
            .arg("--no-fetch")
            .args(&args)
            .current_dir(&r.dir)
            .output()
            .unwrap();
        let rs = Command::new(assert_cmd::cargo::cargo_bin("git-branch-tree"))
            .arg("--no-fetch")
            .args(&args)
            .current_dir(&r.dir)
            .output()
            .unwrap();
        assert_eq!(
            py.status.code(),
            rs.status.code(),
            "exit code differs for {args:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&py.stderr),
            String::from_utf8_lossy(&rs.stderr),
            "stderr differs for {args:?}"
        );
    }
}

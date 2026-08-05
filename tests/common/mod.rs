//! Fixture harness: build small real git repositories in tempdirs.
//!
//! Every integration test drives real git (CI has it); fixtures are milliseconds.
//!
//! The helpers are shared across test binaries, so not every one uses all of them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TestRepo {
    _tmp: tempfile::TempDir,
    pub dir: PathBuf,
}

impl TestRepo {
    /// `git init -b main` with a deterministic identity and one initial commit.
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path().join("repo");
        std::fs::create_dir(&dir).expect("create repo dir");
        let repo = Self { _tmp: tmp, dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.commit_file("README.md", "seed\n", "chore: initial commit");
        repo
    }

    /// Run git in the repo, panicking on failure (fixtures must not silently break).
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.dir, args)
    }

    /// Run git in an arbitrary directory (worktrees, remotes).
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    }

    /// Write `content` to `path` and commit it.
    pub fn commit_file(&self, path: &str, content: &str, msg: &str) {
        let full = self.dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&full, content).expect("write file");
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    /// Create `name` off `from` and check it out.
    pub fn branch_from(&self, name: &str, from: &str) {
        self.git(&["checkout", "-q", "-b", name, from]);
    }

    pub fn checkout(&self, name: &str) {
        self.git(&["checkout", "-q", name]);
    }

    /// Current commit id of a ref.
    pub fn sha(&self, rev: &str) -> String {
        self.git(&["rev-parse", rev])
    }

    /// Squash-merge `branch` into main (content lands under a brand-new commit).
    pub fn squash_merge_into_main(&self, branch: &str) {
        self.checkout("main");
        self.git(&["merge", "--squash", "-q", branch]);
        self.git(&["commit", "-q", "-m", &format!("squash: {branch}")]);
    }

    /// Add a bare clone as `origin` (for base-refresh tests).
    pub fn add_bare_origin(&self) -> PathBuf {
        let bare = self.dir.parent().unwrap().join("origin.git");
        let out = Command::new("git")
            .args([
                "clone",
                "-q",
                "--bare",
                self.dir.to_str().unwrap(),
                bare.to_str().unwrap(),
            ])
            .output()
            .expect("spawn git clone");
        assert!(out.status.success(), "bare clone failed");
        self.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        self.git(&["fetch", "-q", "origin"]);
        bare
    }
}

// ---------------------------------------------------------------------------
// Named fixtures
//
// The shapes below are the domain's recurring cases. Naming them keeps each test
// about the behaviour it asserts rather than about rebuilding a repository, and means
// "the diamond" is one thing everywhere instead of three near-copies.
// ---------------------------------------------------------------------------

/// Branches stacked in git that touch disjoint files, so they are content-independent:
/// feat/a, feat/b, feat/c each add one file. The case the tool exists to flatten.
pub fn disjoint_stack() -> TestRepo {
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

/// A genuine chain: each branch rewrites the line the previous one introduced, so
/// feat/a <- feat/b <- feat/c really is a stack.
pub fn line_edit_chain() -> TestRepo {
    let r = TestRepo::new();
    r.commit_file("f.txt", "l1\nl2\nl3\n", "chore: seed");
    r.branch_from("feat/a", "main");
    r.commit_file("f.txt", "A1\nl2\nl3\n", "feat: a rewrites line 1");
    r.branch_from("feat/b", "feat/a");
    r.commit_file("f.txt", "B1\nl2\nl3\n", "feat: b rewrites line 1");
    r.branch_from("feat/c", "feat/b");
    r.commit_file("f.txt", "C1\nl2\nl3\n", "feat: c rewrites line 1");
    r.checkout("main");
    r
}

/// A diamond rooted at a branch: feat/root *introduces* the file, so left and right
/// both depend on it, and feat/top - which rewrites the lines left and right own -
/// depends on those two, and on root only transitively.
///
///     main -> root -> {left, right} -> top
pub fn diamond_under_root() -> TestRepo {
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

/// The same four branches, but the file comes from the *base*, so root, left and right
/// each edit a line nobody else owns and are independent; only feat/top, which rewrites
/// the lines left and right introduced, has parents.
///
///     main -> {root, left, right}, with top under right (also needing left)
pub fn diamond_on_base() -> TestRepo {
    let r = TestRepo::new();
    r.commit_file("f.txt", "r1\nr2\nr3\n", "chore: seed");
    r.branch_from("feat/root", "main");
    r.commit_file("f.txt", "r1\nR2\nr3\n", "feat: root rewrites line 2");
    r.branch_from("feat/left", "feat/root");
    r.commit_file("f.txt", "L1\nR2\nr3\n", "feat: left rewrites line 1");
    r.branch_from("feat/right", "feat/left");
    r.commit_file("f.txt", "L1\nR2\nG3\n", "feat: right rewrites line 3");
    r.branch_from("feat/top", "feat/right");
    r.commit_file("f.txt", "T1\nR2\nT3\n", "feat: top rewrites lines 1 and 3");
    r.checkout("main");
    r
}

/// The four branch names both diamonds use, in rank order.
pub const DIAMOND: &[&str] = &["feat/root", "feat/left", "feat/right", "feat/top"];

// ---------------------------------------------------------------------------
// Driving the library
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;

use git_branch_tree::blame::SubprocessBlamer;
use git_branch_tree::deps::{Engine, compute_ancestry_dependencies};
use git_branch_tree::exclude::ExcludeSet;
use git_branch_tree::gitx::{Git, RepoView};
use git_branch_tree::model::{BranchSet, build_branches};
use git_branch_tree::patchid::PatchIdCache;

/// The collaborators the pipeline needs, assembled once per fixture.
pub struct Harness {
    pub git: Git,
    pub repo: RepoView,
    pub cache: PatchIdCache,
    pub pool: rayon::ThreadPool,
}

impl Harness {
    pub fn new(r: &TestRepo) -> Self {
        let git = Git::new(&r.dir);
        Self {
            repo: RepoView::discover(&r.dir).expect("open repo"),
            cache: PatchIdCache::new(git.clone()),
            pool: rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .expect("build pool"),
            git,
        }
    }

    pub fn base(&self) -> git_branch_tree::gitx::Sha {
        self.repo.rev_parse("main").expect("resolve main")
    }

    /// Branches built but with no dependency edges computed yet.
    pub fn build(&self, branches: &[&str]) -> BranchSet {
        let names: Vec<String> = branches.iter().map(|s| s.to_string()).collect();
        build_branches(&names, self.base(), &self.repo, &self.cache, &self.pool)
            .expect("build branches")
    }
}

/// Run the full content pipeline and return the resolved graph.
pub fn analyse(r: &TestRepo, branches: &[&str]) -> BranchSet {
    let h = Harness::new(r);
    let mut set = h.build(branches);
    let blamer = SubprocessBlamer { git: h.git.clone() };
    let exclude = ExcludeSet::new(&[], true).expect("default excludes");
    Engine {
        repo: &h.repo,
        git: &h.git,
        blamer: &blamer,
        cache: &h.cache,
        exclude: &exclude,
        pool: &h.pool,
    }
    .compute_dependencies(&mut set, "main", h.base())
    .expect("compute dependencies");
    set
}

/// The same, in `--ancestry` mode.
pub fn analyse_by_ancestry(r: &TestRepo, branches: &[&str]) -> BranchSet {
    let h = Harness::new(r);
    let mut set = h.build(branches);
    compute_ancestry_dependencies(&mut set, &h.repo, &h.pool).expect("compute ancestry");
    set
}

/// `branch -> its dependency parents, sorted by name`, which is what the dependency
/// tests actually assert on.
pub fn parent_map(set: &BranchSet) -> BTreeMap<String, Vec<String>> {
    set.ids()
        .map(|b| {
            let mut parents: Vec<String> = set
                .get(b)
                .parents
                .iter()
                .map(|&p| set.get(p).name.clone())
                .collect();
            parents.sort();
            (set.get(b).name.clone(), parents)
        })
        .collect()
}

/// Build the expected shape of [`parent_map`] from a terse literal.
pub fn expect_parents(pairs: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(b, ps)| (b.to_string(), ps.iter().map(|s| s.to_string()).collect()))
        .collect()
}

/// A set of branch names, for the `merged` arguments the renderers and planner take.
pub fn names(v: &[&str]) -> std::collections::HashSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Driving the binary
// ---------------------------------------------------------------------------

/// Runs the built binary against a fixture.
///
/// Always passes `--no-fetch`, since fixtures have no origin, and redirects the
/// temp directory into the fixture so the test runner's worktrees - which it
/// deliberately leaves behind for reuse - do not litter the real /tmp or outlive the
/// test. Environment is set per child process, never with `set_var`, which would race
/// across the parallel test threads sharing one process environment.
pub struct Gbt {
    cmd: assert_cmd::Command,
}

impl Gbt {
    pub fn new(r: &TestRepo) -> Self {
        let tmp = r.dir.join("tmp");
        std::fs::create_dir_all(&tmp).expect("create fixture tmpdir");
        let mut cmd = assert_cmd::Command::cargo_bin("git-branch-tree").expect("find binary");
        cmd.current_dir(&r.dir)
            .env("TMPDIR", &tmp)
            .arg("--no-fetch");
        Self { cmd }
    }

    /// Run from a different directory, e.g. a subdirectory of the fixture, to check
    /// that the repository is discovered upward.
    pub fn cwd(mut self, dir: &std::path::Path) -> Self {
        self.cmd.current_dir(dir);
        self
    }

    pub fn args(mut self, args: &[&str]) -> Self {
        self.cmd.args(args);
        self
    }

    pub fn env(mut self, key: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        self.cmd.env(key, value);
        self
    }

    /// Expect success; return stdout.
    pub fn stdout(mut self) -> String {
        let out = self.cmd.assert().success();
        String::from_utf8(out.get_output().stdout.clone()).expect("utf8 stdout")
    }

    /// Expect success; return (stdout, stderr) - stderr carries the `# ` notes.
    pub fn output(mut self) -> (String, String) {
        let out = self.cmd.assert().success();
        (
            String::from_utf8(out.get_output().stdout.clone()).expect("utf8 stdout"),
            String::from_utf8(out.get_output().stderr.clone()).expect("utf8 stderr"),
        )
    }

    /// Expect exit 1; return stderr.
    pub fn failure(mut self) -> String {
        let out = self.cmd.assert().failure().code(1);
        String::from_utf8(out.get_output().stderr.clone()).expect("utf8 stderr")
    }

    /// Expect any non-zero exit; return stderr (clap's usage errors exit 2).
    pub fn any_failure(mut self) -> String {
        let out = self.cmd.assert().failure();
        String::from_utf8(out.get_output().stderr.clone()).expect("utf8 stderr")
    }
}

/// Replace the 10-hex short shas in a rebase block, which change whenever a fixture's
/// content does, so assertions stay about structure.
pub fn mask_shas(s: &str) -> String {
    s.split(' ')
        .map(|tok| {
            if tok.len() == 10 && tok.bytes().all(|b| b.is_ascii_hexdigit()) {
                "<SHA>"
            } else {
                tok
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

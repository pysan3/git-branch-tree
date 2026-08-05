//! Support for the tests that drive the built binary.
//!
//! The fixture builder is shared with the unit tests inside `src/` by including the
//! same file, rather than keeping a second copy of it here.
#![allow(dead_code)]

#[path = "../../src/testfix/repo.rs"]
mod repo;
pub use repo::*;

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

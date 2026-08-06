//! Fail fast on missing runtime dependencies.
//!
//! Every check here is cheap. The point is that discovering these late is not: without
//! it, `--auto-merged` with no `gh` installed reports the problem only after refreshing
//! the base over the network and running a patch-id pass over every branch, and `--test`
//! finds an unwritable temp directory after minutes of blame.
//!
//! Two rules shape this module. Checks are conditional on the flags that need them - a
//! run without `--auto-merged` never probes for `gh` - and every problem found is
//! reported together, because fixing one only to be told about the next is a poor trade
//! when reaching the failure costs tens of seconds.

use anyhow::{Result, bail};

use crate::cli::Cli;
use crate::gitx::Git;
use crate::stacks::StackTool;
use crate::util::warn;

/// The oldest git with everything used here. `worktree list --porcelain` arrived in 2.7
/// and is the newest thing relied on; `patch-id --stable` and porcelain blame are older.
const MIN_GIT: (u32, u32) = (2, 7);

/// A problem, paired with what to do about it. The remedy is the useful half: "gh not
/// found" without "install it or drop --auto-merged" just restates the obvious.
struct Problem {
    what: String,
    fix: String,
}

/// Check the runtime dependencies this invocation actually needs.
///
/// Runs before anything expensive - no fetch, no rev-walk - so a missing dependency
/// costs a fraction of a second rather than most of a minute.
pub fn check(cli: &Cli, git: &Git) -> Result<()> {
    let mut problems: Vec<Problem> = Vec::new();

    check_git(cli, git, &mut problems);
    if !cli.no_fetch {
        check_origin(git);
    }
    if cli.auto_merged {
        check_gh(git, &mut problems);
    }
    if let Some(tool) = cli.stack_tool() {
        check_stack_tool(tool, git, &mut problems);
    }
    if let Some(patch) = &cli.test_patch {
        check_patch(patch, &mut problems);
    }
    if cli.test.is_some() {
        check_tmpdir(&mut problems);
    }

    if problems.is_empty() {
        return Ok(());
    }
    let listed: Vec<String> = problems
        .iter()
        .map(|p| format!("  - {}\n    {}", p.what, p.fix))
        .collect();
    let plural = if problems.len() == 1 { "" } else { "s" };
    bail!("unmet requirement{plural}:\n{}", listed.join("\n"));
}

fn check_git(cli: &Cli, git: &Git, problems: &mut Vec<Problem>) {
    let Some((major, minor)) = git_version(git) else {
        problems.push(Problem {
            what: "git was not found, or did not run".into(),
            fix: "install git and make sure it is on PATH".into(),
        });
        return;
    };
    if (major, minor) >= MIN_GIT {
        return;
    }
    let found = format!(
        "git {major}.{minor} is older than the required {}.{}",
        MIN_GIT.0, MIN_GIT.1
    );
    if cli.test.is_some() {
        // --test cannot degrade gracefully: it is worktrees or nothing.
        problems.push(Problem {
            what: found,
            fix: "--test needs `git worktree`; upgrade git or drop --test".into(),
        });
    } else {
        // Everything else still works, so this is worth saying but not worth refusing.
        warn(&format!("{found}; some operations may not work"));
    }
}

/// `git --version` prints `git version X.Y.Z`, possibly with a vendor suffix.
fn git_version(git: &Git) -> Option<(u32, u32)> {
    let out = git.run(&["--version"]).ok()?;
    let rest = out.split_whitespace().nth(2)?;
    let mut parts = rest.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Not an error: without a remote the analysis still runs, just against the local base.
fn check_origin(git: &Git) {
    if !git.ok(&["remote", "get-url", "origin"]) {
        warn("no 'origin' remote, so the base cannot be refreshed; pass --no-fetch to skip");
    }
}

fn check_gh(git: &Git, problems: &mut Vec<Problem>) {
    if git.gh(&["--version"]).is_err() {
        problems.push(Problem {
            what: "--auto-merged needs the gh CLI, which was not found".into(),
            fix: "install gh (https://cli.github.com) or drop --auto-merged".into(),
        });
        return;
    }
    // Presence is not enough: an unauthenticated gh fails on the first query instead.
    if git.gh(&["auth", "status"]).is_err() {
        problems.push(Problem {
            what: "--auto-merged needs gh to be authenticated".into(),
            fix: "run `gh auth login`, or drop --auto-merged".into(),
        });
    }
}

/// The tool must be *usable*, not merely installed: `gh` runs fine without the stack
/// extension and then fails on the one command we need, so each tool names its own probe.
fn check_stack_tool(tool: &dyn StackTool, git: &Git, problems: &mut Vec<Problem>) {
    let spec = tool.spec();
    if git.tool(spec.program, spec.probe_args, spec.env).is_ok() {
        return;
    }
    problems.push(Problem {
        what: format!(
            "{} needs `{} {}`, which was not found or did not run",
            spec.flag,
            spec.program,
            spec.probe_args.join(" ")
        ),
        fix: format!("{}, or drop {}", spec.install, spec.flag),
    });
}

fn check_patch(patch: &std::path::Path, problems: &mut Vec<Problem>) {
    let abs = std::path::absolute(patch).unwrap_or_else(|_| patch.to_path_buf());
    if !abs.is_file() {
        problems.push(Problem {
            what: format!("--test-patch file not found: {}", patch.display()),
            fix: "give a path to an existing patch file".into(),
        });
    }
}

/// The test runner puts its worktrees under the temp directory, so an unwritable one
/// fails every branch - after the whole analysis has already run.
fn check_tmpdir(problems: &mut Vec<Problem>) {
    let root = std::env::temp_dir().join("git-branch-tree");
    let probe = root.join(".write-probe");
    let writable = std::fs::create_dir_all(&root).is_ok() && std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    if !writable {
        problems.push(Problem {
            what: format!("--test needs to write worktrees under {}", root.display()),
            fix: "make that directory writable, or set TMPDIR to somewhere that is".into(),
        });
    }
}

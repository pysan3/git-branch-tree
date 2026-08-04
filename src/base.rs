//! Base branch detection and refresh.

use anyhow::{Result, bail};

use crate::gitx::{Git, RepoView};
use crate::util::warn;

/// Return the base ref, honouring `--base` then falling back to origin/HEAD,
/// then `main` / `master`.
pub fn detect_base(explicit: Option<&str>, repo: &RepoView, git: &Git) -> Result<String> {
    if let Some(base) = explicit {
        if repo.rev_parse(base).is_err() {
            bail!("base ref '{base}' does not exist");
        }
        return Ok(base.to_string());
    }
    if let Ok(head) = git.run(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
        && let Some(name) = head.rsplit('/').next().filter(|n| !n.is_empty())
    {
        return Ok(if repo.branch_exists(name) {
            name.to_string()
        } else {
            format!("origin/{name}")
        });
    }
    for cand in ["main", "master"] {
        if repo.branch_exists(cand) {
            return Ok(cand.to_string());
        }
    }
    bail!("could not auto-detect base branch; pass --base <ref>")
}

/// Path of the worktree that has `branch` checked out, if any.
pub fn worktree_for_branch(git: &Git, branch: &str) -> Option<std::path::PathBuf> {
    let out = git.run(&["worktree", "list", "--porcelain"]).ok()?;
    let mut path: Option<std::path::PathBuf> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.into());
        } else if line == format!("branch refs/heads/{branch}") {
            return path;
        }
    }
    None
}

/// Bring `base` up to date with origin BEFORE analysis, so diffs use a fresh base.
///
/// A local branch checked out in some worktree cannot be updated by a plain fetch
/// (git refuses to move a checked-out ref), so pull inside that worktree; if it is
/// not checked out anywhere, fetch the ref directly. A remote-tracking base
/// (e.g. `origin/master`) is just fetched. Best-effort: warn and continue on failure.
pub fn update_base(git: &Git, base: &str) {
    let ok = if let Some((remote, r)) = base.split_once('/') {
        git.ok(&["fetch", remote, r])
    } else if let Some(wt) = worktree_for_branch(git, base) {
        git.ok_in(&wt, &["pull", "--ff-only", "origin", base])
    } else {
        git.ok(&["fetch", "origin", &format!("{base}:{base}")])
    };
    if !ok {
        warn(&format!(
            "could not update '{base}' from origin; analysing the current '{base}'"
        ));
    }
}

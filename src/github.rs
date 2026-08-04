//! Merged-PR detection for `--auto-merged`.
//!
//! GitHub records a merge regardless of the resulting commit hash, so asking it
//! outright is both simpler and more reliable than reconstructing patch-ids to guess
//! whether a squash-merge landed. It only sees branches that actually had a PR.
//!
//! The `gh` CLI does everything we would otherwise reimplement: resolving the
//! owner/repo from the remote, authenticating via the OS keyring, paging, and the
//! merged-state filter that the REST API does not offer directly (it exposes closed
//! PRs and leaves you to check `merged_at`). Its built-in jq extracts the one field we
//! need, so nothing here parses JSON either.

use std::collections::HashSet;

use anyhow::Result;

use crate::gitx::Git;

/// Of `names`, those whose GitHub PR is already merged.
pub fn detect_merged_prs(git: &Git, names: &[String]) -> Result<Vec<String>> {
    let out = git.gh(&[
        "pr",
        "list",
        "--state",
        "merged",
        "--limit",
        "1000",
        "--json",
        "headRefName",
        "--jq",
        ".[].headRefName",
    ])?;
    let merged: HashSet<&str> = out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    // Preserve the caller's order, and only ever report branches under analysis.
    Ok(names
        .iter()
        .filter(|n| merged.contains(n.as_str()))
        .cloned()
        .collect())
}

//! Merged-PR detection for `--auto-merged`.
//!
//! GitHub records a merge regardless of the resulting commit hash, so asking it
//! outright is both simpler and more reliable than reconstructing patch-ids to guess
//! whether a squash-merge landed. It only sees branches that actually had a PR.
//!
//! Each branch is asked about directly, with `--head`. The obvious alternative - pull
//! the last N merged PRs and intersect - looks cheaper but is wrong: its cost scales
//! with how busy the repository is rather than with how many branches are being
//! analysed, so on an active monorepo the window covers days and every branch merged
//! before it is silently missed. Asking per branch is exact whatever the repository's
//! volume or the PR's age, and the request count is bounded by the branches in play.
//!
//! The `gh` CLI does the rest of what we would otherwise reimplement: resolving the
//! owner/repo from the remote, authenticating via the OS keyring, and the merged-state
//! filter the REST API does not expose directly. Its built-in jq reduces each answer to
//! a count, so nothing here parses JSON.

use anyhow::Result;
use rayon::prelude::*;

use crate::gitx::Git;
use crate::util::note;

/// Of `names`, those whose GitHub PR is already merged.
pub fn detect_merged_prs(
    git: &Git,
    names: &[String],
    pool: &rayon::ThreadPool,
) -> Result<Vec<String>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    note(&format!(
        "asking GitHub whether {} branch(es) have merged pull requests ...",
        names.len()
    ));

    // One request per branch, in parallel; `par_iter` keeps the caller's order.
    let answers: Vec<Result<bool>> = pool.install(|| {
        names
            .par_iter()
            .map(|name| -> Result<bool> {
                // No extra context: the error already renders the whole failing command,
                // `--head <branch>` included, and wrapping it per branch would make the
                // message depend on which parallel query happened to fail first.
                let out = git.gh(&[
                    "pr", "list", "--state", "merged", "--head", name, "--json", "number", "--jq",
                    "length",
                ])?;
                Ok(out.trim().parse::<u32>().unwrap_or(0) > 0)
            })
            .collect()
    });

    let mut merged = Vec::new();
    for (name, answer) in names.iter().zip(answers) {
        if answer? {
            merged.push(name.clone());
        }
    }
    if merged.is_empty() {
        // Say so explicitly: silence here is indistinguishable from the flag not running.
        note("no merged pull requests found for the analysed branches");
    }
    Ok(merged)
}

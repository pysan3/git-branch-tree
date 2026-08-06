//! Input mode resolution: turn CLI branch args / prefixes into concrete branch names.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

use crate::gitx::{Git, RepoView, Sha};
use crate::model::prime_names;
use crate::patchid::{PatchId, PatchIdCache};
use crate::stacks::{self, StackTool};

/// Leading run of ASCII letters, e.g. `PROJ-412` -> `PROJ`, `pysan3/foo` -> `pysan`.
pub fn alpha_key(s: &str) -> &str {
    let end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    &s[..end]
}

/// Whether `branch` matches any of `prefixes` (literal, or by leading-letter group).
pub fn prefix_matches(branch: &str, prefixes: &[String], alpha: bool) -> bool {
    if alpha {
        let key = alpha_key(branch);
        prefixes.iter().any(|p| alpha_key(p) == key)
    } else {
        prefixes.iter().any(|p| branch.starts_with(p.as_str()))
    }
}

/// Where the branch list comes from.
///
/// Picking the mode belongs to [`crate::cli::Cli`], which owns the flags. Keeping it
/// there means nothing below re-derives the user's intent from which arguments happen
/// to be non-empty - the shape that let `--prefix P feat/a` silently drop `feat/a`.
#[derive(Debug)]
pub enum InputMode<'a> {
    Prefix { prefixes: &'a [String], alpha: bool },
    StackedOn(&'a str),
    Explicit(&'a [String]),
    Tool(&'static dyn StackTool),
}

/// Resolve the concrete list of branch names to analyse.
pub fn resolve_branches(
    mode: InputMode<'_>,
    base: Sha,
    base_ref: &str,
    repo: &RepoView,
    git: &Git,
    cache: &PatchIdCache,
    pool: &rayon::ThreadPool,
) -> Result<Vec<String>> {
    match mode {
        InputMode::Prefix { prefixes, alpha } => by_prefix(repo, prefixes, alpha),
        InputMode::StackedOn(root) => stacked_on(root, base, repo, cache, pool),
        InputMode::Explicit(names) => explicit(names, repo),
        InputMode::Tool(tool) => from_tool(tool, base_ref, repo, git),
    }
}

/// The branches an external stack tool declares, minus the base.
pub fn from_tool(
    tool: &'static dyn StackTool,
    base_ref: &str,
    repo: &RepoView,
    git: &Git,
) -> Result<Vec<String>> {
    let locals = repo.local_branches()?;
    let named = stacks::branches(tool, git, &locals)?;
    // A tool that draws its trunk as part of the stack would otherwise hand back the
    // base as a branch to analyse, making it a node depending on itself. `detect_base`
    // can answer `origin/master` where the tool prints `master`, so drop both spellings.
    let base_short = base_ref.rsplit('/').next().unwrap_or(base_ref);
    let mut seen = BTreeSet::new();
    let names: Vec<String> = named
        .into_iter()
        .filter(|n| n != base_ref && n != base_short)
        .filter(|n| seen.insert(n.clone()))
        .collect();
    if names.is_empty() {
        bail!(
            "{}: the stack holds nothing but the base branch ({base_ref})",
            tool.spec().flag
        );
    }
    Ok(names)
}

/// Every local branch matching any prefix (or `--alpha` leading-letter group).
pub fn by_prefix(repo: &RepoView, prefixes: &[String], alpha: bool) -> Result<Vec<String>> {
    let names: Vec<String> = repo
        .local_branches()?
        .into_iter()
        .filter(|b| prefix_matches(b, prefixes, alpha))
        .collect();
    if names.is_empty() {
        let how = if alpha {
            "leading-letter group"
        } else {
            "prefix(es)"
        };
        bail!("no local branches match {how}: {}", prefixes.join(", "));
    }
    Ok(names)
}

/// `root` plus every local branch stacked on it - i.e. carrying all of its changes *by
/// content* (patch-ids), not by ancestry.
///
/// The only mode needing the base, the patch-id cache and the pool: it patch-ids every
/// local branch to answer the subset test.
pub fn stacked_on(
    root: &str,
    base: Sha,
    repo: &RepoView,
    cache: &PatchIdCache,
    pool: &rayon::ThreadPool,
) -> Result<Vec<String>> {
    if !repo.branch_exists(root) {
        bail!("branch '{root}' does not exist");
    }
    let locals = repo.local_branches()?;
    // One batched patch-id pass over every local branch, then subset tests.
    let lists = prime_names(repo, base, &locals, cache, pool)?;
    let patchset = |name: &str| -> BTreeSet<PatchId> {
        lists
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|&s| cache.get(s))
            .collect()
    };
    let root_pids = patchset(root);
    let mut result: BTreeSet<&str> = BTreeSet::from([root]);
    // b is stacked on root iff it carries all of root's changes (content, not sha).
    for b in &locals {
        if b != root && !root_pids.is_empty() && root_pids.is_subset(&patchset(b)) {
            result.insert(b);
        }
    }
    Ok(result.into_iter().map(String::from).collect())
}

/// Exactly the branches named, deduped, caller order kept.
pub fn explicit(branches: &[String], repo: &RepoView) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for b in branches {
        if !repo.branch_exists(b) {
            bail!("branch '{b}' does not exist");
        }
        if seen.insert(b.as_str()) {
            names.push(b.clone());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_key_takes_leading_letters() {
        assert_eq!(alpha_key("PROJ-412"), "PROJ");
        assert_eq!(alpha_key("pysan3/foo"), "pysan");
        assert_eq!(alpha_key("123-x"), "");
    }

    #[test]
    fn prefix_matching_literal_and_alpha() {
        let prefixes = vec!["PROJ-41".to_string(), "OPS-7".to_string()];
        assert!(prefix_matches("PROJ-412/feat", &prefixes, false));
        assert!(!prefix_matches("PROJ-500/feat", &prefixes, false));
        assert!(prefix_matches("PROJ-500/feat", &prefixes, true));
        assert!(prefix_matches("OPS-999", &prefixes, true));
        assert!(!prefix_matches("OTHER-1", &prefixes, true));
    }
}

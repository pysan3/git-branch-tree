//! Content-based dependency detection (the default mode).
//!
//! 1. Diff every branch against its chain-upstream and blame the OLD-side line
//!    ranges (bounded to `base..prev`) to find which branch introduced the lines it
//!    edits — a real dependency.
//! 2. A branch identical in content to its chain parent still depends on it.
//! 3. Content-containment: a branch carrying another's identical NEW files (absent
//!    from the base) without a git-ancestry link genuinely depends on it.
//! 4. Transitive reduction keeps only the nearest dependencies.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use rayon::prelude::*;

use crate::exclude::ExcludeSet;
use crate::gitx::{Git, RepoView, Sha};
use crate::hunks::{diff_unified0, parse_old_side_hunks};
use crate::model::{BranchId, BranchSet};

use super::Engine;
use super::reduce::transitive_reduction;

struct BlameJob {
    branch: BranchId,
    prevref: String,
    path: String,
    lo: u32,
    hi: u32,
}

/// Diff a branch against its chain-upstream and expand into per-hunk blame jobs.
/// A branch sitting directly on the base has no upstream branch to depend on, so it
/// is skipped entirely (no diff, no blame).
fn branch_blame_jobs(
    set: &BranchSet,
    x: BranchId,
    git: &Git,
    exclude: &ExcludeSet,
) -> Result<Vec<BlameJob>> {
    let Some(prev) = set.get(x).prev else {
        return Ok(Vec::new());
    };
    let prevref = set.get(prev).name.clone();
    let diff = diff_unified0(git, &prevref, &set.get(x).name)?;
    let mut jobs = Vec::new();
    for (path, hunks) in parse_old_side_hunks(&diff) {
        if exclude.is_excluded(&path) {
            continue;
        }
        for (start, count) in hunks {
            // For an insertion (count 0) blame the surrounding lines instead.
            let (lo, hi) = if count == 0 {
                (start, start + 1)
            } else {
                (start, start + count - 1)
            };
            jobs.push(BlameJob {
                branch: x,
                prevref: prevref.clone(),
                path: path.clone(),
                lo,
                hi,
            });
        }
    }
    Ok(jobs)
}

/// Map each file a branch changes (vs its merge-base with the base) to its final
/// blob (`None` = deleted). Baseline-independent and commit-shape-independent, so it
/// catches identical carried content even when commits and patch-ids differ.
fn contribution(
    repo: &RepoView,
    tip: Sha,
    base: Sha,
    exclude: &ExcludeSet,
) -> Result<BTreeMap<String, Option<Sha>>> {
    let mb = repo.merge_base(base, tip).unwrap_or(base);
    let mut blobs = BTreeMap::new();
    for change in repo.raw_diff(mb, tip)? {
        if !exclude.is_excluded(&change.path) {
            blobs.insert(change.path, change.blob);
        }
    }
    Ok(blobs)
}

impl Engine<'_> {
    /// Populate each branch's `parents` set with the branches it truly depends on.
    pub fn compute_dependencies(
        &self,
        set: &mut BranchSet,
        base_name: &str,
        base: Sha,
    ) -> Result<()> {
        let (repo, git, blamer, cache, exclude, pool) = (
            self.repo,
            self.git,
            self.blamer,
            self.cache,
            self.exclude,
            self.pool,
        );
        // Map every own-commit's patch-id to the branch that introduced it (first wins,
        // in input order).
        let mut pid_owner = HashMap::new();
        for b in set.ids() {
            for &pid in &set.get(b).own_pids {
                pid_owner.entry(pid).or_insert(b);
            }
        }

        // Phase 1 (parallel): diff every branch against its upstream -> flat job list.
        let ids: Vec<BranchId> = set.ids().collect();
        let jobs: Vec<BlameJob> = pool
            .install(|| {
                ids.par_iter()
                    .map(|&x| branch_blame_jobs(set, x, git, exclude))
                    .collect::<Result<Vec<_>>>()
            })?
            .into_iter()
            .flatten()
            .collect();

        // Phase 2 (parallel): blame each hunk's OLD range to find introducing commits.
        let blamed: Vec<(BranchId, HashSet<Sha>)> = pool.install(|| {
            jobs.par_iter()
                .map(|j| {
                    (
                        j.branch,
                        blamer.blame_range(&j.prevref, base_name, &j.path, j.lo, j.hi),
                    )
                })
                .collect()
        });

        // Collect blame shas per branch, then resolve their patch-ids in one batch.
        let mut per_branch: HashMap<BranchId, HashSet<Sha>> = HashMap::new();
        let mut all_shas: Vec<Sha> = Vec::new();
        for (branch, shas) in blamed {
            all_shas.extend(shas.iter().copied());
            per_branch.entry(branch).or_default().extend(shas);
        }
        all_shas.sort();
        all_shas.dedup();
        cache.prime(&all_shas)?;

        for x in set.ids() {
            let mut add = Vec::new();
            for &sha in per_branch.get(&x).into_iter().flatten() {
                if let Some(pid) = cache.get(sha)
                    && let Some(&owner) = pid_owner.get(&pid)
                    && owner != x
                {
                    add.push(owner);
                }
            }
            let b = set.get_mut(x);
            b.parents.extend(add);
            // A branch identical in content to its chain parent still depends on it.
            if b.parents.is_empty()
                && b.own_shas.is_empty()
                && let Some(prev) = b.prev
            {
                b.parents.insert(prev);
            }
        }

        // Content-dependency edges via identical NEW files (absent from the base). A
        // merely-modified file already in the base is dropped by the rebase, so carrying
        // it is not a real dependency (edits to it are caught by blame above).
        let contribs: Vec<BTreeMap<String, Option<Sha>>> = pool.install(|| {
            ids.par_iter()
                .map(|&x| contribution(repo, set.get(x).tip, base, exclude))
                .collect::<Result<_>>()
        })?;
        let mut all_files: Vec<&String> = contribs.iter().flat_map(|c| c.keys()).collect();
        all_files.sort();
        all_files.dedup();
        let in_base: HashMap<&String, bool> = pool.install(|| {
            all_files
                .par_iter()
                .map(|&f| (f, repo.path_in_tree(base, f)))
                .collect()
        });
        let new_files: Vec<HashSet<&String>> = contribs
            .iter()
            .map(|c| {
                c.keys()
                    .filter(|f| !in_base.get(f).copied().unwrap_or(false))
                    .collect()
            })
            .collect();

        for &x in &ids {
            let nx = &new_files[x.0];
            for &y in &ids {
                let ny = &new_files[y.0];
                // Y must introduce new files and X must introduce them all too.
                if y == x || ny.is_empty() || set.get(x).parents.contains(&y) || !ny.is_subset(nx) {
                    continue;
                }
                // Identical new-file sets: keep one direction only.
                if ny == nx && set.rank(y) >= set.rank(x) {
                    continue;
                }
                // Require at least one byte-identical shared new file.
                if !ny
                    .iter()
                    .any(|f| contribs[x.0].get(*f) == contribs[y.0].get(*f))
                {
                    continue;
                }
                // New files carried purely via git-ancestry are dropped by the rebase, so
                // that is a sibling; a non-ancestor carrying them genuinely depends on Y.
                if !repo.is_ancestor(set.get(y).tip, set.get(x).tip) {
                    set.get_mut(x).parents.insert(y);
                }
            }
        }

        transitive_reduction(set);
        Ok(())
    }
}

//! The rebase plan: where each still-open branch rebases to, and from which commit.

use std::collections::{BTreeSet, HashSet};

use crate::gitx::Sha;
use crate::model::{BranchId, BranchSet};
use crate::patchid::PatchId;

/// One rebase step.
#[derive(Debug)]
pub struct PlanEntry {
    pub branch: BranchId,
    /// Ref the branch lands on: nearest still-open dependency, or the base.
    pub onto: String,
    /// Upstream/skip commit: only its successors are replayed.
    pub up: Sha,
    /// Further open dependencies a single linear rebase onto `onto` cannot satisfy.
    pub extra: Vec<String>,
}

/// Compute, per non-merged branch, where it rebases.
///
/// `onto` is the nearest still-open dependency, or the base when every dependency is
/// merged - flattening away ancestry that is not a real dependency. For a branch
/// landing on the base, `up` is the NEWEST already-merged commit in the branch's
/// history, so every merged commit is skipped (squash-merges give landed code a new
/// hash, so an ancestor's commits still sit above the base here and would otherwise
/// be re-applied and conflict with the squashed version).
pub fn rebase_plan(
    set: &BranchSet,
    base: &str,
    merged: &HashSet<String>,
    merged_pids: &BTreeSet<PatchId>,
) -> Vec<PlanEntry> {
    let mut plan = Vec::new();
    for b in set.ids_by_rank() {
        let branch = set.get(b);
        if merged.contains(&branch.name) {
            continue;
        }
        let Some(prev) = branch.prev else {
            continue; // independent: sits directly on the base, nothing to flatten
        };
        let mut open_parents: Vec<BranchId> = branch
            .parents
            .iter()
            .filter(|&&p| !merged.contains(&set.get(p).name))
            .copied()
            .collect();
        open_parents.sort_by(|&a, &b| set.rank(b).cmp(&set.rank(a))); // rank desc
        let onto = open_parents
            .first()
            .map(|&p| set.get(p).name.clone())
            .unwrap_or_else(|| base.to_string());

        let mut up = set.get(prev).tip;
        if onto == base {
            // Skip all merged commits: up = newest commit whose patch-id is merged.
            for &sha in &branch.all_shas {
                // oldest-first, so the last hit wins
                if branch
                    .pid_map
                    .get(&sha)
                    .is_some_and(|p| merged_pids.contains(p))
                {
                    up = sha;
                }
            }
        }

        let extra: Vec<String> = match open_parents.split_first() {
            Some((&first, rest)) => {
                let first_pids = &set.get(first).pidset;
                let mut names: Vec<String> = rest
                    .iter()
                    .filter(|&&p| {
                        let pids = &set.get(p).pidset;
                        // Drop parents already subsumed by the chosen onto (strict subset).
                        !(pids.is_subset(first_pids) && pids.len() < first_pids.len())
                    })
                    .map(|&p| set.get(p).name.clone())
                    .collect();
                names.sort();
                names
            }
            None => Vec::new(),
        };
        plan.push(PlanEntry {
            branch: b,
            onto,
            up,
            extra,
        });
    }
    plan
}

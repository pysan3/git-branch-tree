//! Pure git-ancestry dependency mode (`--ancestry`).
//!
//! When the branches already form a real stack (each rebased onto the previous),
//! plain `merge-base --is-ancestor` is exact and needs none of the patch-id / blame
//! / content heuristics (which exist for rebased or squash-scrambled histories).

use anyhow::Result;
use rayon::prelude::*;

use crate::gitx::RepoView;
use crate::model::{BranchId, BranchSet};

use super::reduce::transitive_reduction;

pub fn compute_ancestry_dependencies(
    set: &mut BranchSet,
    repo: &RepoView,
    pool: &rayon::ThreadPool,
) -> Result<()> {
    let ids: Vec<BranchId> = set.ids().collect();
    let pairs: Vec<(BranchId, BranchId)> = ids
        .iter()
        .flat_map(|&x| ids.iter().filter(move |&&y| y != x).map(move |&y| (y, x)))
        .collect();
    let is_anc: Vec<bool> = pool.install(|| {
        pairs
            .par_iter()
            .map(|&(y, x)| repo.is_ancestor(set.get(y).tip, set.get(x).tip))
            .collect()
    });
    let anc_map: std::collections::HashSet<(BranchId, BranchId)> = pairs
        .iter()
        .zip(&is_anc)
        .filter(|&(_, &a)| a)
        .map(|(&p, _)| p)
        .collect();
    for &(y, x) in &pairs {
        // y is an ancestor of x -> x depends on y. Guard the equal-tip case (mutual
        // ancestry) with rank so it cannot form a 2-cycle.
        if anc_map.contains(&(y, x)) && (!anc_map.contains(&(x, y)) || set.rank(y) < set.rank(x)) {
            set.get_mut(x).parents.insert(y);
        }
    }
    transitive_reduction(set);
    Ok(())
}

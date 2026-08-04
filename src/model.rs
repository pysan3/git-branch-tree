//! The branch model: per-branch content identity and the dependency graph arena.
//!
//! Graph edges are `BranchId` indices into a [`BranchSet`] (the Rust analogue of the
//! Python original's object-identity edges): `Copy` ids, deterministic `BTreeSet`
//! ordering, and free `Send`/`Sync` for the parallel phases.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use rayon::prelude::*;

use crate::gitx::{RepoView, Sha};
use crate::patchid::{PatchId, PatchIdCache};

/// Index of a branch inside its [`BranchSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchId(pub usize);

#[derive(Debug)]
pub struct Branch {
    pub name: String,
    pub tip: Sha,
    /// Commits in `base..tip`, oldest first.
    pub all_shas: Vec<Sha>,
    pub pid_map: HashMap<Sha, PatchId>,
    pub pidset: BTreeSet<PatchId>,
    /// Nearest chain-upstream branch (`None` == sits directly on the base).
    pub prev: Option<BranchId>,
    /// Commits whose content is unique to this branch, oldest first.
    pub own_shas: Vec<Sha>,
    pub own_pids: BTreeSet<PatchId>,
    /// Dependency parents (empty == hangs off the base).
    pub parents: BTreeSet<BranchId>,
}

pub struct BranchSet {
    pub branches: Vec<Branch>,
}

impl BranchSet {
    pub fn get(&self, id: BranchId) -> &Branch {
        &self.branches[id.0]
    }

    pub fn get_mut(&mut self, id: BranchId) -> &mut Branch {
        &mut self.branches[id.0]
    }

    pub fn ids(&self) -> impl Iterator<Item = BranchId> + use<> {
        (0..self.branches.len()).map(BranchId)
    }

    pub fn by_name(&self, name: &str) -> Option<BranchId> {
        self.branches
            .iter()
            .position(|b| b.name == name)
            .map(BranchId)
    }

    /// Ordering key: upstream branches (fewer own-content commits) sort first.
    pub fn rank(&self, id: BranchId) -> (usize, &str) {
        let b = self.get(id);
        (b.pidset.len(), b.name.as_str())
    }

    /// Ids sorted by [`Self::rank`].
    pub fn ids_by_rank(&self) -> Vec<BranchId> {
        let mut ids: Vec<BranchId> = self.ids().collect();
        ids.sort_by(|&a, &b| self.rank(a).cmp(&self.rank(b)));
        ids
    }
}

/// `git rev-list base..name` for every name in parallel (newest-first).
pub fn rev_lists(
    repo: &RepoView,
    base: Sha,
    names: &[String],
    pool: &rayon::ThreadPool,
) -> Result<HashMap<String, Vec<Sha>>> {
    let lists: Vec<(String, Vec<Sha>)> = pool.install(|| {
        names
            .par_iter()
            .map(|n| -> Result<(String, Vec<Sha>)> {
                let tip = repo.rev_parse(n)?;
                Ok((n.clone(), repo.rev_list(base, tip)?))
            })
            .collect::<Result<_>>()
    })?;
    Ok(lists.into_iter().collect())
}

/// Populate the patch-id cache for every commit of every branch in one batch.
pub fn prime_names(
    repo: &RepoView,
    base: Sha,
    names: &[String],
    cache: &PatchIdCache,
    pool: &rayon::ThreadPool,
) -> Result<HashMap<String, Vec<Sha>>> {
    let lists = rev_lists(repo, base, names, pool)?;
    let mut all: Vec<Sha> = lists.values().flatten().copied().collect();
    all.sort();
    all.dedup();
    cache.prime(&all)?;
    Ok(lists)
}

/// Construct the arena and work out each branch's nearest upstream + own commits.
pub fn build_branches(
    names: &[String],
    base: Sha,
    repo: &RepoView,
    cache: &PatchIdCache,
    pool: &rayon::ThreadPool,
) -> Result<BranchSet> {
    let mut lists = prime_names(repo, base, names, cache, pool)?;
    let mut branches = Vec::with_capacity(names.len());
    for name in names {
        let tip = repo.rev_parse(name)?;
        let mut all_shas = lists.remove(name).unwrap_or_default();
        all_shas.reverse(); // oldest first
        let pid_map: HashMap<Sha, PatchId> = all_shas
            .iter()
            .filter_map(|&s| cache.get(s).map(|p| (s, p)))
            .collect();
        let pidset: BTreeSet<PatchId> = pid_map.values().copied().collect();
        branches.push(Branch {
            name: name.clone(),
            tip,
            all_shas,
            pid_map,
            pidset,
            prev: None,
            own_shas: Vec::new(),
            own_pids: BTreeSet::new(),
            parents: BTreeSet::new(),
        });
    }
    let mut set = BranchSet { branches };

    // prev = the branch whose pidset is the largest strict subset of ours.
    for x in set.ids() {
        let mut best: Option<BranchId> = None;
        for other in set.ids() {
            if other == x {
                continue;
            }
            let o = set.get(other);
            if o.pidset.is_empty() || !o.pidset.is_subset(&set.get(x).pidset) {
                continue;
            }
            if o.pidset.len() == set.get(x).pidset.len() {
                continue; // strict subset only
            }
            if best.is_none_or(|b| o.pidset.len() > set.get(b).pidset.len()) {
                best = Some(other);
            }
        }
        let prev_pids = best.map(|b| set.get(b).pidset.clone()).unwrap_or_default();
        let b = set.get_mut(x);
        b.prev = best;
        b.own_shas = b
            .all_shas
            .iter()
            .filter(|s| b.pid_map.get(*s).is_some_and(|p| !prev_pids.contains(p)))
            .copied()
            .collect();
        b.own_pids = b
            .own_shas
            .iter()
            .filter_map(|s| b.pid_map.get(s))
            .copied()
            .collect();
    }
    Ok(set)
}

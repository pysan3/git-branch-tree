//! Test support: fixtures, plus helpers that drive the pipeline directly.
//!
//! Lives in `src/` so the tests that need the internals can reach them without any of
//! it being `pub` to the outside world - which is the whole reason this crate publishes
//! no library API.
#![allow(dead_code)]

mod repo;
pub use repo::*;

// ---------------------------------------------------------------------------
// Driving the library
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;

use crate::blame::SubprocessBlamer;
use crate::deps::{Engine, compute_ancestry_dependencies};
use crate::exclude::ExcludeSet;
use crate::gitx::{Git, RepoView};
use crate::model::{BranchSet, build_branches};
use crate::patchid::PatchIdCache;

/// The collaborators the pipeline needs, assembled once per fixture.
pub struct Harness {
    pub git: Git,
    pub repo: RepoView,
    pub cache: PatchIdCache,
    pub pool: rayon::ThreadPool,
}

impl Harness {
    pub fn new(r: &TestRepo) -> Self {
        let git = Git::new(&r.dir);
        Self {
            repo: RepoView::discover(&r.dir).expect("open repo"),
            cache: PatchIdCache::new(git.clone()),
            pool: rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .expect("build pool"),
            git,
        }
    }

    pub fn base(&self) -> crate::gitx::Sha {
        self.repo.rev_parse("main").expect("resolve main")
    }

    /// Branches built but with no dependency edges computed yet.
    pub fn build(&self, branches: &[&str]) -> BranchSet {
        let names: Vec<String> = branches.iter().map(|s| s.to_string()).collect();
        build_branches(&names, self.base(), &self.repo, &self.cache, &self.pool)
            .expect("build branches")
    }
}

/// Run the full content pipeline and return the resolved graph.
pub fn analyse(r: &TestRepo, branches: &[&str]) -> BranchSet {
    let h = Harness::new(r);
    let mut set = h.build(branches);
    let blamer = SubprocessBlamer { git: h.git.clone() };
    let exclude = ExcludeSet::new(&[], true).expect("default excludes");
    Engine {
        repo: &h.repo,
        git: &h.git,
        blamer: &blamer,
        cache: &h.cache,
        exclude: &exclude,
        pool: &h.pool,
    }
    .compute_dependencies(&mut set, "main", h.base())
    .expect("compute dependencies");
    set
}

/// The same, in `--ancestry` mode.
pub fn analyse_by_ancestry(r: &TestRepo, branches: &[&str]) -> BranchSet {
    let h = Harness::new(r);
    let mut set = h.build(branches);
    compute_ancestry_dependencies(&mut set, &h.repo, &h.pool).expect("compute ancestry");
    set
}

/// `branch -> its dependency parents, sorted by name`, which is what the dependency
/// tests actually assert on.
pub fn parent_map(set: &BranchSet) -> BTreeMap<String, Vec<String>> {
    set.ids()
        .map(|b| {
            let mut parents: Vec<String> = set
                .get(b)
                .parents
                .iter()
                .map(|&p| set.get(p).name.clone())
                .collect();
            parents.sort();
            (set.get(b).name.clone(), parents)
        })
        .collect()
}

/// Build the expected shape of [`parent_map`] from a terse literal.
pub fn expect_parents(pairs: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(b, ps)| (b.to_string(), ps.iter().map(|s| s.to_string()).collect()))
        .collect()
}

/// A set of branch names, for the `merged` arguments the renderers and planner take.
pub fn names(v: &[&str]) -> std::collections::HashSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

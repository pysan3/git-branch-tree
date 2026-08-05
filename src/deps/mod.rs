//! Dependency-edge computation: content heuristics (default) or pure git ancestry.

pub mod ancestry;
pub mod content;
pub mod reduce;

pub use ancestry::compute_ancestry_dependencies;
pub use reduce::transitive_reduction;

use crate::blame::Blamer;
use crate::exclude::ExcludeSet;
use crate::gitx::{Git, RepoView};
use crate::patchid::PatchIdCache;

/// Everything the content engine needs to answer questions about a repository.
///
/// These six travel together through every phase - and through the caller that
/// assembles them - so they are one value rather than six parameters repeated at each
/// hop. The phases then take only what actually varies between them.
pub struct Engine<'a> {
    pub repo: &'a RepoView,
    pub git: &'a Git,
    pub blamer: &'a dyn Blamer,
    pub cache: &'a PatchIdCache,
    pub exclude: &'a ExcludeSet,
    pub pool: &'a rayon::ThreadPool,
}

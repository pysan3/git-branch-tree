//! git-branch-tree - discover the real merge-dependency tree of a stack of branches.
//!
//! Branches are usually stacked linearly (A off master, B off A, C off B, ...), but the
//! real dependencies are often flatter: if A only touches file X and B only touches
//! file Y, B does not actually depend on A even though it was branched off it. This
//! crate works out the true dependency DAG by looking at the *content* each branch
//! changes - not git ancestry - so it is robust to rebases and squash-merges (where
//! the same change gets a different commit hash).

pub mod app;
pub mod base;
pub mod blame;
pub mod cli;
pub mod deps;
pub mod exclude;
pub mod gitx;
pub mod hunks;
pub mod input;
pub mod model;
pub mod patchid;
pub mod plan;
pub mod render;
pub mod suffix;
pub mod util;

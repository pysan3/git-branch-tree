//! git-branch-tree - discover the real merge-dependency tree of a stack of branches.
//!
//! Branches are usually stacked linearly (A off master, B off A, C off B, ...), but the
//! real dependencies are often flatter: if A only touches file X and B only touches
//! file Y, B does not actually depend on A even though it was branched off it. This
//! crate works out the true dependency DAG by looking at the *content* each branch
//! changes - not git ancestry - so it is robust to rebases and squash-merges (where
//! the same change gets a different commit hash).

pub mod util;

/// Run the whole pipeline. The binary maps `Err` to `error: ...` on stderr + exit 1.
pub fn run() -> anyhow::Result<()> {
    anyhow::bail!("not implemented yet")
}

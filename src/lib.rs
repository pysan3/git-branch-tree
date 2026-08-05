//! git-branch-tree - discover the real merge-dependency tree of a stack of branches.
//!
//! Branches are usually stacked linearly (A off master, B off A, C off B, ...), but the
//! real dependencies are often flatter: if A only touches file X and B only touches
//! file Y, B does not actually depend on A even though it was branched off it. This
//! crate works out the true dependency DAG by looking at the *content* each branch
//! changes - not git ancestry - so it is robust to rebases and squash-merges (where
//! the same change gets a different commit hash).
//!
//! # This library is not a public API
//!
//! It exists so the binary and the integration tests can share code, and everything in
//! it is `pub` only because integration tests are separate crates. **Nothing here is
//! covered by semantic versioning**: types, signatures and whole modules may change or
//! disappear in any release, including a patch one.
//!
//! Two concrete reasons it cannot be promised. `gix` types appear throughout - `Sha` is
//! an alias for [`gix::ObjectId`] - and `gix` is itself pre-1.0 and releases breaking
//! versions roughly monthly, so every bump would otherwise be a breaking change here.
//! `rayon::ThreadPool` appears in signatures for the same reason.
//!
//! The supported interface is the **command line**: its flags, its output, and the
//! rebase block it emits. Those are pinned by tests and are what the version number
//! speaks for. If you want to drive this from Rust, open an issue and say what for -
//! a deliberate API can then be designed rather than inferred from internals.

#[doc(hidden)]
pub mod app;
#[doc(hidden)]
pub mod base;
#[doc(hidden)]
pub mod blame;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod deps;
#[doc(hidden)]
pub mod exclude;
#[doc(hidden)]
pub mod github;
#[doc(hidden)]
pub mod gitx;
#[doc(hidden)]
pub mod hunks;
#[doc(hidden)]
pub mod input;
#[doc(hidden)]
pub mod model;
#[doc(hidden)]
pub mod patchid;
#[doc(hidden)]
pub mod plan;
#[doc(hidden)]
pub mod preflight;
#[doc(hidden)]
pub mod render;
#[doc(hidden)]
pub mod suffix;
#[doc(hidden)]
pub mod testrun;
#[doc(hidden)]
pub mod util;

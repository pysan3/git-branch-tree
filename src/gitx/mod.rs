//! Git access layer.
//!
//! Read-only plumbing (refs, rev-walks, merge-bases, tree diffs) goes through
//! [`repo::RepoView`] (gix, in-process). Porcelain that only real git does faithfully
//! (blame bounded to a commit range, worktrees, rebase, fetch) goes through the
//! [`cmd::Git`] subprocess façade.

pub mod cmd;
pub mod repo;

pub use cmd::{CmdError, Git};
pub use repo::{RawChange, RepoView, Sha};

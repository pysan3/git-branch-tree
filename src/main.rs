//! git-branch-tree - discover the real merge-dependency tree of a stack of branches.
//!
//! Branches are usually stacked linearly (A off master, B off A, C off B, ...), but the
//! real dependencies are often flatter: if A only touches file X and B only touches
//! file Y, B does not actually depend on A even though it was branched off it. This
//! works out the true dependency DAG by looking at the *content* each branch changes -
//! not git ancestry - so it is robust to rebases and squash-merges, where the same
//! change gets a different commit hash.
//!
//! There is deliberately no library target. Everything below is private to the binary,
//! so nothing here is a published API and no signature is a promise - which matters
//! because gix types appear throughout and gix is pre-1.0. The supported interface is
//! the command line: its flags, its output, and the rebase block it emits.

use clap::Parser;

mod app;
mod base;
mod blame;
mod cli;
mod deps;
mod exclude;
mod github;
mod gitx;
mod hunks;
mod input;
mod model;
mod patchid;
mod plan;
mod preflight;
mod render;
mod stacks;
mod suffix;
mod testrun;
mod util;

#[cfg(test)]
mod testfix;
#[cfg(test)]
mod tests;

// SIGINT keeps its default disposition: the process dies of the signal and the shell
// reports exit 130, and child git/test processes in the same group receive it too.
fn main() {
    if let Err(err) = app::run(cli::Cli::parse()) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

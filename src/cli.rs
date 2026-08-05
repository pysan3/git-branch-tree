//! Command-line surface.
//!
//! No colour anywhere: the report is meant to be copy-pasted (the rebase block in
//! particular), and escape codes would come along for the ride.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::suffix::{SuffixConfig, SuffixTemplate};

/// Which tree rendering(s) to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Ascii,
    Mermaid,
    Both,
}

/// Default worker count: oversubscribe cores a little, since the work is IO bound on
/// git subprocesses rather than CPU bound.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .saturating_mul(2)
        .min(32)
}

const LONG_HELP: &str = "\
INPUT MODES
  git-branch-tree <branch>                 one branch: analyse it + every local branch
                                           stacked on top of it (branched off it)
  git-branch-tree <b1> <b2> <b3> ...       many branches: analyse exactly those
  git-branch-tree --prefix PROJ-123         prefix: all local branches starting PROJ-123
  git-branch-tree --prefix PROJ-1 PROJ-2     several prefixes are unioned
  git-branch-tree --alpha --prefix PROJ-123 match the leading-letter group only, so
                                           PROJ-123 selects every PROJ-* branch

HOW IT WORKS
  1. Resolve the branches to analyse and a base branch (default: auto-detected
     main/master).
  2. Isolate each branch's own commits with `git patch-id`, a content hash, so a
     branch merely carrying an ancestor's commits is not credited with that work.
  3. Blame the exact lines each branch's own commits change, in the parent revision,
     to find which branch originally introduced them. A change tracing back to a
     branch in the set is a real dependency; one tracing back to the base is not.
  4. Draw an edge U -> X for every branch U that X depends on, then drop transitive
     edges so each node hangs off its nearest dependencies only. A node may have
     several parents; a branch with no dependency hangs off the base.
  5. Emit the tree and a copy-pasteable `git rebase --onto` block.

Before analysing, the base is refreshed from origin so every diff and merge-base is
computed against the latest master. This is worktree-aware: if the base is checked out
in another worktree it is pulled there, since a plain fetch cannot move a checked-out
ref; otherwise the ref is fetched directly. Skip it with --no-fetch.

The rebase block is emitted as a single `&&` chain (bookended with `true`) so it can be
pasted and run in one go, stopping at the first command that fails.

Blame is bounded to the base..<parent> range, so even a frequently-churned file never
drags the base's full history into the analysis; branches sitting directly on the base
are answered from their diff alone, with no blame at all.
";

#[derive(Debug, Parser)]
#[command(
    name = "git-branch-tree",
    version,
    about = "Discover the real merge-dependency tree of a stack of branches.",
    after_long_help = LONG_HELP
)]
pub struct Cli {
    /// branch name(s)
    pub branches: Vec<String>,

    /// analyse all local branches with any of these prefixes
    /// (e.g. --prefix PROJ-12{3..5})
    #[arg(long, num_args = 1.., value_name = "PREFIX")]
    pub prefix: Vec<String>,

    /// match --prefix by leading-letter group only, so 'PROJ-123', 'ABC-7' and
    /// 'alice/wip' select every PROJ-*, ABC-* and alice* branch
    #[arg(long)]
    pub alpha: bool,

    /// build the tree from pure git ancestry instead of the content heuristics
    /// (exact when the branches are already cleanly stacked on each other)
    #[arg(long)]
    pub ancestry: bool,

    /// base branch (default: auto-detect main/master)
    #[arg(long)]
    pub base: Option<String>,

    /// branches squash-merged into base (space- or comma-separated)
    #[arg(long, num_args = 1.., value_name = "BRANCH")]
    pub merged: Vec<String>,

    /// tree format
    #[arg(long, value_enum, default_value_t = Format::Mermaid)]
    pub format: Format,

    /// parallel git workers (default: 2x CPU cores, capped at 32)
    #[arg(short = 'j', long, default_value_t = default_jobs())]
    pub jobs: usize,

    /// extra path globs to ignore for dependency detection (e.g. '*.snap')
    #[arg(long, num_args = 1.., value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// do not skip the built-in generated/lock files
    #[arg(long)]
    pub no_default_exclude: bool,

    /// also treat as --merged any analysed branch whose GitHub PR is already merged
    /// (via `gh pr list`; needs the gh CLI + network)
    #[arg(long)]
    pub auto_merged: bool,

    /// omit branches that depend on >1 still-unmerged parent from the rebase block
    /// (they cannot be rebased onto a single branch); list them as a comment
    #[arg(long)]
    pub skip_ambiguous: bool,

    /// shell command run against base+branch for each branch that would land on the
    /// base; if it exits non-zero the branch is omitted from the rebase block and listed
    /// (catches semantic deps - e.g. calling code from an unmerged branch)
    #[arg(long, value_name = "CMD")]
    pub test: Option<String>,

    /// worker count for --test [default: 1]. Each worker holds its own worktree - a
    /// full checkout, plus whatever the build writes into it - so raise this
    /// deliberately once you know what one costs on disk. Serial also lets tests share
    /// a single Bazel --output_base; a shared --disk_cache is usually better, since it
    /// reuses artifacts while staying parallel
    #[arg(long, value_name = "N")]
    pub test_jobs: Option<usize>,

    /// git-apply this patch to each worktree after the rebase and before --test (local,
    /// uncommitted fixes - e.g. machine-specific - that make tests pass)
    #[arg(long, value_name = "PATH")]
    pub test_patch: Option<PathBuf>,

    /// do not update the base from origin before analysing (offline / local base)
    #[arg(long)]
    pub no_fetch: bool,

    /// do not print the rebase command block
    #[arg(long)]
    pub no_rebase: bool,

    /// command appended with `&&` for each branch landing on the base (repeatable;
    /// replaces the default `review`). Placeholders: {branch} {onto} {base} {up};
    /// {{ and }} are literal braces. Pass an empty value to append nothing
    #[arg(long = "on-base", value_name = "CMD")]
    pub on_base: Option<Vec<SuffixTemplate>>,

    /// command appended with `&&` for each branch landing on a still-open parent
    /// (repeatable; replaces the default `gh pr edit {branch} --base {onto}`)
    #[arg(long = "on-parent", value_name = "CMD")]
    pub on_parent: Option<Vec<SuffixTemplate>>,
}

impl Cli {
    /// At least one worker, however the flag was given.
    pub fn job_count(&self) -> usize {
        self.jobs.max(1)
    }

    /// Workers for `--test`. Defaults to one, unlike `-j`: a git worker costs a
    /// subprocess, but a test worker costs an entire worktree plus whatever the build
    /// writes into it, which on a large repository is gigabytes apiece. Parallel test
    /// runs are worth opting into, not stumbling into.
    pub fn test_job_count(&self) -> usize {
        self.test_jobs.unwrap_or(1).max(1)
    }

    /// `--merged` accepts both space- and comma-separated names, so the flag can be
    /// pasted from a commit message or a PR list.
    pub fn merged_names(&self) -> Vec<String> {
        self.merged
            .iter()
            .flat_map(|m| m.split([',', ' ']))
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    pub fn wants_ascii(&self) -> bool {
        matches!(self.format, Format::Ascii | Format::Both)
    }

    pub fn wants_mermaid(&self) -> bool {
        matches!(self.format, Format::Mermaid | Format::Both)
    }

    /// The per-branch suffix commands, defaults applied. Templates were already
    /// validated by clap, so this cannot fail on a bad placeholder.
    pub fn suffixes(&self) -> anyhow::Result<SuffixConfig> {
        SuffixConfig::from_cli(self.on_base.as_deref(), self.on_parent.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("git-branch-tree").chain(args.iter().copied()))
    }

    #[test]
    fn merged_accepts_commas_and_spaces() {
        assert_eq!(
            parse(&["--merged", "a,b", "c d"]).merged_names(),
            vec!["a", "b", "c", "d"]
        );
        assert!(parse(&["x"]).merged_names().is_empty());
    }

    #[test]
    fn mermaid_is_the_default_format() {
        let cli = parse(&["feat/x"]);
        assert!(cli.wants_mermaid());
        assert!(!cli.wants_ascii());

        let cli = parse(&["--format", "both", "feat/x"]);
        assert!(cli.wants_mermaid() && cli.wants_ascii());
    }

    #[test]
    fn jobs_is_at_least_one() {
        assert_eq!(parse(&["-j", "0", "x"]).job_count(), 1);
        assert_eq!(parse(&["-j", "7", "x"]).job_count(), 7);
        assert_eq!(parse(&["x"]).job_count(), default_jobs());
    }

    #[test]
    fn test_jobs_defaults_to_one_regardless_of_j() {
        // Not tied to -j: a test worker costs a whole worktree, not a subprocess.
        assert_eq!(parse(&["x"]).test_job_count(), 1);
        assert_eq!(parse(&["-j", "16", "x"]).test_job_count(), 1);
        assert_eq!(parse(&["--test-jobs", "4", "x"]).test_job_count(), 4);
        assert_eq!(parse(&["--test-jobs", "0", "x"]).test_job_count(), 1);
    }

    #[test]
    fn several_prefixes_and_branches_parse() {
        let cli = parse(&["--prefix", "PROJ-1", "PROJ-2", "--alpha"]);
        assert_eq!(cli.prefix, vec!["PROJ-1", "PROJ-2"]);
        assert!(cli.alpha);
        assert!(cli.branches.is_empty());
    }
}

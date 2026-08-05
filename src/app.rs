//! Pipeline orchestrator: refresh base, resolve branches, build the graph, plan the
//! rebase, print the report.
//!
//! Output discipline: stdout carries the report only, stderr carries `# `-prefixed
//! notes. The report is printed last so it lands at the bottom of the terminal,
//! unburied by whatever the operational phases wrote.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};

use crate::base::{detect_base, update_base};
use crate::blame::SubprocessBlamer;
use crate::cli::Cli;
use crate::deps::{Engine, compute_ancestry_dependencies};
use crate::exclude::ExcludeSet;
use crate::github::detect_merged_prs;
use crate::gitx::{Git, RepoView};
use crate::input::resolve_branches;
use crate::model::build_branches;
use crate::patchid::{PatchId, PatchIdCache};
use crate::plan::{PlanEntry, rebase_plan};
use crate::render::{render_ascii, render_header, render_mermaid, render_rebase};
use crate::testrun::run_tests;

pub fn run(cli: Cli) -> Result<()> {
    // Resolve the suffix templates before touching git, so a bad placeholder fails
    // immediately rather than after a minute of blame.
    let suffixes = cli.suffixes()?;

    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let repo = RepoView::discover(&cwd)?;
    let workdir = repo.work_dir()?;
    let git = Git::new(&workdir);

    // Before the first expensive step, so a missing dependency costs a moment rather
    // than most of a minute.
    crate::preflight::check(&cli, &git)?;

    let base = detect_base(cli.base.as_deref(), &repo, &git)?;
    // Refresh the base FIRST, so every merge-base and diff below is computed against
    // the latest upstream state rather than a stale local ref.
    if !cli.no_fetch {
        update_base(&git, &base);
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cli.job_count())
        .build()
        .context("cannot start the worker pool")?;
    let cache = PatchIdCache::new(git.clone());
    let base_sha = repo.rev_parse(&base)?;

    let mut merged = cli.merged_names();
    let names = resolve_branches(
        &cli.branches,
        &cli.prefix,
        cli.alpha,
        base_sha,
        &repo,
        &cache,
        &pool,
    )?;

    // Ask GitHub before building the graph, so a network failure costs nothing.
    let mut auto: Vec<String> = Vec::new();
    if cli.auto_merged {
        auto = detect_merged_prs(&git, &names, &pool)?
            .into_iter()
            .filter(|n| !merged.contains(n))
            .collect();
        merged.extend(auto.iter().cloned());
    }

    let mut set = build_branches(&names, base_sha, &repo, &cache, &pool)?;
    if cli.ancestry {
        compute_ancestry_dependencies(&mut set, &repo, &pool)?;
    } else {
        let blamer = SubprocessBlamer { git: git.clone() };
        let exclude = ExcludeSet::new(&cli.exclude, !cli.no_default_exclude)?;
        let engine = Engine {
            repo: &repo,
            git: &git,
            blamer: &blamer,
            cache: &cache,
            exclude: &exclude,
            pool: &pool,
        };
        engine.compute_dependencies(&mut set, &base, base_sha)?;
    }

    // Only branches actually under analysis can be resolved to patch-ids, so a
    // --merged name that is not in the set contributes nothing to the skip point.
    merged.retain(|m| set.by_name(m).is_some());
    let merged_set: HashSet<String> = merged.iter().cloned().collect();

    // Tests run BEFORE the report is printed: they may write a lot of output, and the
    // tree and rebase block should land at the bottom of the terminal, not be buried.
    let mut plan: Vec<PlanEntry> = Vec::new();
    let mut failed: HashMap<String, String> = HashMap::new();
    if !cli.no_rebase {
        // Patch-ids of everything already merged, so the rebase `up` skips all of it.
        let merged_pids: BTreeSet<PatchId> = set
            .ids()
            .filter(|&b| merged_set.contains(&set.get(b).name))
            .flat_map(|b| set.get(b).pidset.iter().copied())
            .collect();
        plan = rebase_plan(&set, &base, &merged_set, &merged_pids);

        if let Some(cmd) = &cli.test {
            // Existence was checked by preflight; this only makes the path absolute,
            // since it is applied from inside a worktree.
            let patch = cli
                .test_patch
                .as_ref()
                .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()));
            failed = run_tests(
                &set,
                &plan,
                &base,
                &git,
                cmd,
                cli.test_job_count(),
                patch.as_deref(),
            )?;
        }
    }

    print!("{}", render_header(&set, &base, &auto));
    println!("\n");
    if cli.wants_mermaid() {
        println!("{}\n", render_mermaid(&set, &base, &merged_set));
    }
    if cli.wants_ascii() {
        println!("{}\n", render_ascii(&set, &base, &merged_set));
    }
    if !cli.no_rebase {
        println!(
            "{}",
            render_rebase(
                &set,
                &plan,
                &base,
                &merged_set,
                cli.skip_ambiguous,
                &failed,
                &suffixes,
            )
        );
    }
    Ok(())
}

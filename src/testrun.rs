//! `--test`: prove a branch really can stand on the base, by actually rebasing it.
//!
//! A branch the graph says can land on the base may still not *work* there: it might
//! call a function another open branch defines, which no textual diff or blame can see.
//! So for every base-targeted branch we perform the exact rebase the tool would emit -
//! on a detached HEAD in a throwaway worktree, so no branch ref moves - and then run the
//! user's command. A rebase conflict or a non-zero exit means the branch is not
//! self-contained on the base, and it gets dropped from the emitted chain.
//!
//! Worktrees live at predictable, reused paths under
//! `<tmp>/git-branch-tree/<repo-slug>/wt-N`, so disk stays capped at
//! `min(jobs, branches)` checkouts and a repeat run reuses each checkout - which keeps
//! build caches warm instead of paying for a cold build every time. They are left in
//! place afterwards, for reuse and for inspecting a failure.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::gitx::Git;
use crate::model::BranchSet;
use crate::plan::PlanEntry;
use crate::util::{note, warn};

/// Predictable per-repository directory holding the reusable test worktrees.
fn worktree_root(git: &Git) -> Result<PathBuf> {
    let common = git
        .run(&["rev-parse", "--git-common-dir"])
        .context("cannot locate the git directory")?;
    let raw = Path::new(&common);
    let abs = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        git.dir().join(raw)
    };
    // Canonicalise so the slug is the same whichever subdirectory the tool was run from.
    let abs = abs.canonicalize().unwrap_or(abs);

    let mut slug = String::new();
    for ch in abs.to_string_lossy().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_').to_string();
    Ok(std::env::temp_dir().join("git-branch-tree").join(slug))
}

/// Run the user's command through a shell, inheriting stdio so they see its output.
fn shell(cmd: &str, dir: &Path) -> std::io::Result<std::process::ExitStatus> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    c.current_dir(dir).status()
}

/// Reset a worktree to `name`'s tip, discarding whatever the last run left behind.
fn reset_to(git: &Git, wt: &Path, name: &str) {
    // An interrupted previous run can leave a rebase in progress, which would make
    // every later git command in this worktree fail.
    git.ok_in_noninteractive(wt, &["rebase", "--abort"]);
    git.ok_in_noninteractive(wt, &["checkout", "-f", "--detach", name]);
    git.ok_in_noninteractive(wt, &["reset", "--hard", name]);
    git.ok_in_noninteractive(wt, &["clean", "-fdq"]);
}

/// Test each base-targeted branch; return `{branch: reason}` for the ones that failed.
pub fn run_tests(
    set: &BranchSet,
    plan: &[PlanEntry],
    base: &str,
    git: &Git,
    cmd: &str,
    jobs: usize,
    patch: Option<&Path>,
) -> Result<HashMap<String, String>> {
    // Only branches that would land directly on the base can be tested: one landing on
    // a parent is meant to depend on unmerged code.
    let targets: Vec<(String, String)> = plan
        .iter()
        .filter(|e| e.onto == base)
        .map(|e| (set.get(e.branch).name.clone(), e.up.to_string()))
        .collect();
    if targets.is_empty() {
        return Ok(HashMap::new());
    }

    let n = jobs.min(targets.len()).max(1);
    let root = worktree_root(git)?;
    std::fs::create_dir_all(&root).with_context(|| format!("cannot create {}", root.display()))?;
    // Clear registrations for worktrees whose directories are gone, or `worktree add`
    // below will refuse the path.
    git.ok(&["worktree", "prune"]);

    let mut slots: Vec<PathBuf> = Vec::with_capacity(n);
    for i in 0..n {
        let wt = root.join(format!("wt-{i}"));
        if !git.ok_in(&wt, &["rev-parse", "--is-inside-work-tree"]) {
            // Not a usable worktree: clear whatever is there and register a fresh one.
            let _ = std::fs::remove_dir_all(&wt);
            git.run(&[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                &wt.to_string_lossy(),
                base,
            ])
            .with_context(|| format!("cannot create test worktree {}", wt.display()))?;
        }
        slots.push(wt);
    }
    note(&format!(
        "test worktrees under {} (reused across runs)",
        root.display()
    ));

    let queue = Mutex::new(VecDeque::from(targets));
    let failed: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());

    // Each worker owns one worktree for the whole run, so two rebases never share a
    // checkout; work is handed out from a single queue so a slow branch cannot idle the
    // others.
    std::thread::scope(|scope| {
        for wt in &slots {
            scope.spawn(|| {
                loop {
                    let Some((name, up)) = queue.lock().unwrap().pop_front() else {
                        break;
                    };
                    reset_to(git, wt, &name);
                    // Exactly the rebase the emitted block performs, on a detached HEAD.
                    let rebased = git.ok_in_noninteractive(wt, &["rebase", "--onto", base, &up]);
                    if !rebased {
                        git.ok_in_noninteractive(wt, &["rebase", "--abort"]);
                        failed
                            .lock()
                            .unwrap()
                            .insert(name, "does not apply cleanly onto the base".to_string());
                        continue;
                    }
                    if let Some(patch) = patch
                        && !git.ok_in(wt, &["apply", &patch.to_string_lossy()])
                    {
                        warn(&format!(
                            "--test-patch did not apply to {name}; testing without it"
                        ));
                    }
                    note(&format!("testing {name} in {} ...", wt.display()));
                    match shell(cmd, wt) {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            let code = status
                                .code()
                                .map_or_else(|| "signal".to_string(), |c| c.to_string());
                            failed
                                .lock()
                                .unwrap()
                                .insert(name, format!("test command failed (exit {code})"));
                        }
                        Err(e) => {
                            failed
                                .lock()
                                .unwrap()
                                .insert(name, format!("could not run the test command: {e}"));
                        }
                    }
                }
            });
        }
    });

    Ok(failed.into_inner().unwrap())
}

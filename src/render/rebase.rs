//! The copy-pasteable rebase command block.

use std::collections::{HashMap, HashSet};

use crate::model::BranchSet;
use crate::plan::PlanEntry;
use crate::suffix::{SuffixConfig, SuffixCtx};
use crate::util::short;

/// Emit the `git rebase --onto` block that flattens the stack onto the base.
///
/// All commands are joined into one `&&` chain (bookended with `true`) so the whole
/// block can be pasted and run at once, stopping at the first failure. The base is
/// refreshed before analysis, so no fetch is embedded here. Branches depending on more
/// than one still-open parent (`--skip-ambiguous`), or that failed the `--test`
/// command, are omitted from the chain and listed instead; a branch stacked on a failed
/// branch is left alone too, since its code is unvalidated until the failing dependency
/// lands.
pub fn render_rebase(
    set: &BranchSet,
    plan: &[PlanEntry],
    base: &str,
    merged: &HashSet<String>,
    skip_ambiguous: bool,
    failed: &HashMap<String, String>,
    suffixes: &SuffixConfig,
) -> String {
    let mut notes: Vec<String> = Vec::new();
    let mut cmds: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut failing: Vec<String> = Vec::new();

    if !merged.is_empty() {
        let mut names: Vec<&str> = merged.iter().map(String::as_str).collect();
        names.sort();
        notes.push(format!(
            "# squash-merged branches skipped: {}",
            names.join(", ")
        ));
    }

    // A failed branch taints everything that depends on it, transitively. `plan` is
    // ordered dependencies-first, but a fixpoint pass is order-independent and cheap.
    let mut tainted: HashMap<String, String> = failed.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for entry in plan {
            let name = &set.get(entry.branch).name;
            if tainted.contains_key(name) {
                continue;
            }
            let blocker = set
                .get(entry.branch)
                .parents
                .iter()
                .map(|&p| set.get(p).name.as_str())
                .find(|p| tainted.contains_key(*p));
            if let Some(blocker) = blocker {
                tainted.insert(
                    name.clone(),
                    format!("stacked on {blocker} which failed --test"),
                );
                changed = true;
            }
        }
    }

    for entry in plan {
        let name = &set.get(entry.branch).name;
        if let Some(reason) = tainted.get(name) {
            failing.push(format!("#   {name}  ({reason})"));
            continue;
        }
        if !entry.extra.is_empty() && skip_ambiguous {
            skipped.push(format!(
                "#   {name}  (also needs: {})",
                entry.extra.join(", ")
            ));
            continue;
        }
        if !entry.extra.is_empty() {
            notes.push(format!(
                "# NOTE: {name} also depends on {}; rebasing it onto {} may conflict \
                 until those merge (or drop it with --skip-ambiguous)",
                entry.extra.join(", "),
                entry.onto
            ));
        }
        // Rebase, then push the rewritten branch, then whatever the user configured for
        // this landing kind: by default a branch reaching the base is ready to ship,
        // while one landing on a parent is a stacked PR needing its base retargeted.
        let up = entry.up.to_string();
        let mut cmd = format!(
            "git rebase --onto {} {} {name} && git checkout {name} && git push --force-with-lease",
            entry.onto,
            short(&up),
        );
        let templates = if entry.onto == base {
            &suffixes.on_base
        } else {
            &suffixes.on_parent
        };
        let ctx = SuffixCtx {
            branch: name,
            onto: &entry.onto,
            base,
            up: short(&up),
        };
        for t in templates {
            cmd.push_str(" && ");
            cmd.push_str(&t.expand(&ctx));
        }
        cmds.push(cmd);
    }

    if !failing.is_empty() {
        notes.push(
            "# left alone (failed --test, or stacked on a failed branch; rerun later):".to_string(),
        );
        notes.append(&mut failing);
    }
    if !skipped.is_empty() {
        notes.push("# skipped (depend on >1 unmerged parent; rerun once they merge):".to_string());
        notes.append(&mut skipped);
    }

    let mut lines = notes;
    if cmds.is_empty() {
        lines.push("# (nothing to rebase - every branch already sits on the base)".to_string());
    } else {
        // One && chain, bookended with `true` so every real command carries a leading &&.
        let chain: Vec<String> = std::iter::once("true".to_string())
            .chain(cmds.into_iter().map(|c| format!("&& {c}")))
            .chain(std::iter::once("&& true".to_string()))
            .collect();
        lines.push(chain.join(" \\\n"));
    }
    lines.join("\n")
}

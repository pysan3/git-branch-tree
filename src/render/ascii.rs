//! ASCII tree rendering of the dependency forest, rooted at the base.

use std::collections::HashSet;

use crate::model::{BranchId, BranchSet};

use super::{display_parents, display_primary};

/// Render the dependency forest as an indented ASCII tree rooted at the base.
///
/// The graph is a DAG (and, in tangled repos, may even carry a cycle remnant), so:
/// top-level entries are real roots first, then any node not reachable from them;
/// a node with multiple parents hangs under its primary parent annotated with
/// `(also depends on: ...)`; a node reached twice renders once and subsequent
/// occurrences become `<name>  ↩ (shown above)`.
pub fn render_ascii(set: &BranchSet, base: &str, merged: &HashSet<String>) -> String {
    let shown: Vec<BranchId> = set
        .ids()
        .filter(|&b| !merged.contains(&set.get(b).name))
        .collect();

    let mut children: Vec<Vec<BranchId>> = vec![Vec::new(); set.branches.len()];
    for &b in &shown {
        if let Some(parent) = display_primary(set, b, merged)
            && parent != b
        {
            children[parent.0].push(b);
        }
    }
    for kids in &mut children {
        kids.sort_by(|&a, &b| set.get(a).name.cmp(&set.get(b).name));
    }

    // Top-level entries: real roots first, then any node not reachable from them (a
    // cycle remnant), so every branch prints exactly once and no cycle can loop.
    let mut reachable = vec![false; set.branches.len()];
    fn mark(children: &[Vec<BranchId>], reachable: &mut [bool], n: BranchId) {
        if reachable[n.0] {
            return;
        }
        reachable[n.0] = true;
        for &c in &children[n.0] {
            mark(children, reachable, c);
        }
    }
    let mut order: Vec<BranchId> = shown.clone();
    order.sort_by(|&a, &b| {
        let ka = (!display_parents(set, a, merged).is_empty(), set.rank(a));
        let kb = (!display_parents(set, b, merged).is_empty(), set.rank(b));
        ka.cmp(&kb)
    });
    let mut entries = Vec::new();
    for b in order {
        if !reachable[b.0] {
            entries.push(b);
            mark(&children, &mut reachable, b);
        }
    }

    let mut lines = vec![base.to_string()];
    let mut printed = vec![false; set.branches.len()];

    #[allow(clippy::too_many_arguments)]
    fn walk(
        set: &BranchSet,
        children: &[Vec<BranchId>],
        merged: &HashSet<String>,
        printed: &mut [bool],
        lines: &mut Vec<String>,
        node: BranchId,
        prefix: &str,
        is_last: bool,
    ) {
        let connector = if is_last { "└─ " } else { "├─ " };
        let name = &set.get(node).name;
        if printed[node.0] {
            // DAG cross-link / cycle: show once, reference otherwise.
            lines.push(format!("{prefix}{connector}{name}  ↩ (shown above)"));
            return;
        }
        printed[node.0] = true;
        let dp = display_parents(set, node, merged);
        let extra = if dp.len() > 1 {
            let primary = display_primary(set, node, merged);
            let mut others: Vec<&str> = dp
                .iter()
                .filter(|&&p| Some(p) != primary)
                .map(|&p| set.get(p).name.as_str())
                .collect();
            others.sort();
            format!("   (also depends on: {})", others.join(", "))
        } else {
            String::new()
        };
        lines.push(format!("{prefix}{connector}{name}{extra}"));
        let child_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
        let kids = &children[node.0];
        for (i, &c) in kids.iter().enumerate() {
            walk(
                set,
                children,
                merged,
                printed,
                lines,
                c,
                &child_prefix,
                i == kids.len() - 1,
            );
        }
    }

    let n = entries.len();
    for (i, &r) in entries.iter().enumerate() {
        walk(
            set,
            &children,
            merged,
            &mut printed,
            &mut lines,
            r,
            "",
            i == n - 1,
        );
    }
    lines.join("\n")
}

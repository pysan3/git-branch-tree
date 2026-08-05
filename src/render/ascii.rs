//! ASCII tree rendering of the dependency forest, rooted at the base.

use std::collections::HashSet;

use crate::model::{BranchId, BranchSet};

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
        if let Some(parent) = set.primary_open_parent(b, merged)
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
        let ka = (!set.open_parents(a, merged).is_empty(), set.rank(a));
        let kb = (!set.open_parents(b, merged).is_empty(), set.rank(b));
        ka.cmp(&kb)
    });
    let mut entries = Vec::new();
    for b in order {
        if !reachable[b.0] {
            entries.push(b);
            mark(&children, &mut reachable, b);
        }
    }

    let tree = Tree {
        set,
        children: &children,
        merged,
    };
    tree.render(&entries, base)
}

/// The read-only inputs every step of the walk consults. Grouped because they are
/// fixed for the whole traversal; the parts that change per step - which node, how
/// deep, and the output being built - stay explicit arguments.
struct Tree<'a> {
    set: &'a BranchSet,
    children: &'a [Vec<BranchId>],
    merged: &'a HashSet<String>,
}

impl Tree<'_> {
    fn render(&self, roots: &[BranchId], base: &str) -> String {
        let mut printed = vec![false; self.set.branches.len()];
        let mut lines = vec![base.to_string()];
        for (i, &r) in roots.iter().enumerate() {
            self.walk(r, "", i == roots.len() - 1, &mut printed, &mut lines);
        }
        lines.join("\n")
    }

    fn walk(
        &self,
        node: BranchId,
        prefix: &str,
        is_last: bool,
        printed: &mut [bool],
        lines: &mut Vec<String>,
    ) {
        let connector = if is_last { "└─ " } else { "├─ " };
        let name = &self.set.get(node).name;
        if printed[node.0] {
            // DAG cross-link / cycle: show once, reference otherwise.
            lines.push(format!("{prefix}{connector}{name}  ↩ (shown above)"));
            return;
        }
        printed[node.0] = true;
        lines.push(format!(
            "{prefix}{connector}{name}{}",
            self.annotation(node)
        ));

        let child_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
        let kids = &self.children[node.0];
        for (i, &c) in kids.iter().enumerate() {
            self.walk(c, &child_prefix, i == kids.len() - 1, printed, lines);
        }
    }

    /// Names the dependencies a node has beyond the parent it is drawn under, since the
    /// tree can only nest it below one of them.
    fn annotation(&self, node: BranchId) -> String {
        let open = self.set.open_parents(node, self.merged);
        if open.len() <= 1 {
            return String::new();
        }
        let primary = self.set.primary_open_parent(node, self.merged);
        let mut others: Vec<&str> = open
            .iter()
            .filter(|&&p| Some(p) != primary)
            .map(|&p| self.set.get(p).name.as_str())
            .collect();
        others.sort();
        format!("   (also depends on: {})", others.join(", "))
    }
}

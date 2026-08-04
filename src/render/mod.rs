//! Renderers: report header, ASCII tree and Mermaid graph.
//!
//! Both tree renderers are merged-aware: an already-merged branch is omitted (it
//! *is* the base now) and any open branch that depended only on merged branches
//! hangs directly off the base.

pub mod ascii;
pub mod mermaid;

use std::collections::HashSet;

use crate::model::{BranchId, BranchSet};

pub use ascii::render_ascii;
pub use mermaid::render_mermaid;

/// Dependency parents excluding merged ones (which have collapsed into the base).
pub(crate) fn display_parents(
    set: &BranchSet,
    b: BranchId,
    merged: &HashSet<String>,
) -> Vec<BranchId> {
    set.get(b)
        .parents
        .iter()
        .filter(|&&p| !merged.contains(&set.get(p).name))
        .copied()
        .collect()
}

/// Nearest non-merged parent, or `None` when the branch now hangs off the base.
pub(crate) fn display_primary(
    set: &BranchSet,
    b: BranchId,
    merged: &HashSet<String>,
) -> Option<BranchId> {
    display_parents(set, b, merged)
        .into_iter()
        .max_by(|&a, &b| set.rank(a).cmp(&set.rank(b)))
}

/// The report header printed above the trees.
pub fn render_header(set: &BranchSet, base: &str, auto_merged: &[String]) -> String {
    let names: Vec<&str> = set.branches.iter().map(|b| b.name.as_str()).collect();
    let mut lines = vec![
        format!("# base: {base}"),
        format!("# branches ({}): {}", names.len(), names.join(", ")),
    ];
    if !auto_merged.is_empty() {
        let mut sorted = auto_merged.to_vec();
        sorted.sort();
        lines.push(format!(
            "# auto-detected as merged on GitHub: {}",
            sorted.join(", ")
        ));
    }
    lines.join("\n")
}

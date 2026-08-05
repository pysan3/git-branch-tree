//! Renderers: report header, ASCII tree, Mermaid graph, rebase command block.
//!
//! Both tree renderers are merged-aware: an already-merged branch is omitted (it
//! *is* the base now) and any open branch that depended only on merged branches
//! hangs directly off the base.

pub mod ascii;
pub mod mermaid;
pub mod rebase;

use crate::model::BranchSet;

pub use ascii::render_ascii;
pub use mermaid::render_mermaid;
pub use rebase::render_rebase;

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

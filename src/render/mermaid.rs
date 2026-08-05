//! Mermaid graph rendering (handles multiple parents natively).

use std::collections::HashSet;

use crate::model::{BranchId, BranchSet};

fn nid(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("b_{sanitized}")
}

/// Render the dependency DAG as a fenced Mermaid `graph TD` block. Merged branches
/// are omitted; branches that depended only on them point at the base.
pub fn render_mermaid(set: &BranchSet, base: &str, merged: &HashSet<String>) -> String {
    let shown: Vec<BranchId> = set
        .ids()
        .filter(|&b| !merged.contains(&set.get(b).name))
        .collect();

    let mut lines = vec![
        "```mermaid".to_string(),
        "graph TD".to_string(),
        format!("  {}([\"{base}\"])", nid(base)),
    ];
    for &b in &shown {
        let name = &set.get(b).name;
        lines.push(format!("  {}[\"{name}\"]", nid(name)));
    }
    for &b in &shown {
        let name = &set.get(b).name;
        let dp = set.open_parents(b, merged);
        if dp.is_empty() {
            lines.push(format!("  {} --> {}", nid(base), nid(name)));
        } else {
            let mut parents: Vec<&str> = dp.iter().map(|&p| set.get(p).name.as_str()).collect();
            parents.sort();
            for p in parents {
                lines.push(format!("  {} --> {}", nid(p), nid(name)));
            }
        }
    }
    lines.push("```".to_string());
    lines.join("\n")
}

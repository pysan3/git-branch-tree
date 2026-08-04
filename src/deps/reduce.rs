//! Transitive reduction over the branch DAG (keep only nearest dependencies).

use crate::model::{BranchId, BranchSet};

/// True if `anc` is reachable by walking `node`'s parent edges upward. Iterative
/// with a visited set, so cycles (which tangled repos can produce) terminate.
pub fn graph_is_ancestor(set: &BranchSet, anc: BranchId, node: BranchId) -> bool {
    let mut seen = vec![false; set.branches.len()];
    let mut stack: Vec<BranchId> = set.get(node).parents.iter().copied().collect();
    while let Some(p) = stack.pop() {
        if p == anc {
            return true;
        }
        if !seen[p.0] {
            seen[p.0] = true;
            stack.extend(set.get(p).parents.iter().copied());
        }
    }
    false
}

/// Drop edge U -> X when U is also an ancestor of another parent of X.
pub fn transitive_reduction(set: &mut BranchSet) {
    for x in set.ids() {
        let parents: Vec<BranchId> = set.get(x).parents.iter().copied().collect();
        for u in parents {
            let keeps_via_other = set
                .get(x)
                .parents
                .iter()
                .any(|&other| other != u && graph_is_ancestor(set, u, other));
            if keeps_via_other {
                set.get_mut(x).parents.remove(&u);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitx::Sha;
    use crate::model::{Branch, BranchSet};
    use std::collections::BTreeSet;

    fn fake_set(n: usize, edges: &[(usize, usize)]) -> BranchSet {
        let mut branches: Vec<Branch> = (0..n)
            .map(|i| Branch {
                name: format!("b{i}"),
                tip: Sha::null(gix::hash::Kind::Sha1),
                all_shas: Vec::new(),
                pid_map: Default::default(),
                pidset: Default::default(),
                prev: None,
                own_shas: Vec::new(),
                own_pids: Default::default(),
                parents: BTreeSet::new(),
            })
            .collect();
        for &(parent, child) in edges {
            branches[child].parents.insert(BranchId(parent));
        }
        BranchSet { branches }
    }

    #[test]
    fn reduces_chain_to_nearest_edges() {
        // 0 -> 1 -> 2 plus the transitive 0 -> 2, which must be dropped.
        let mut set = fake_set(3, &[(0, 1), (1, 2), (0, 2)]);
        transitive_reduction(&mut set);
        assert_eq!(set.get(BranchId(2)).parents, BTreeSet::from([BranchId(1)]));
        assert_eq!(set.get(BranchId(1)).parents, BTreeSet::from([BranchId(0)]));
    }

    #[test]
    fn keeps_diamond_edges() {
        // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3: both edges into 3 are nearest.
        let mut set = fake_set(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
        transitive_reduction(&mut set);
        assert_eq!(
            set.get(BranchId(3)).parents,
            BTreeSet::from([BranchId(1), BranchId(2)])
        );
    }

    #[test]
    fn terminates_on_cycles() {
        let mut set = fake_set(2, &[(0, 1), (1, 0)]);
        transitive_reduction(&mut set);
        // No hang; both edges survive (neither is transitive via a third node).
        assert!(!set.get(BranchId(0)).parents.is_empty());
        assert!(!set.get(BranchId(1)).parents.is_empty());
    }
}

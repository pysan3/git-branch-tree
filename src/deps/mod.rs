//! Dependency-edge computation: content heuristics (default) or pure git ancestry.

pub mod ancestry;
pub mod content;
pub mod reduce;

pub use ancestry::compute_ancestry_dependencies;
pub use content::compute_dependencies;
pub use reduce::transitive_reduction;

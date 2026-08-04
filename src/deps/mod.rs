//! Dependency-edge computation.

pub mod content;
pub mod reduce;

pub use content::compute_dependencies;
pub use reduce::transitive_reduction;

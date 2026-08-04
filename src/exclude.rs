//! Path exclusion for dependency detection.
//!
//! Generated / state / lock files: changes there are not real code dependencies, and
//! blaming huge churned files (e.g. Terraform state) is slow. Skipped by default;
//! disable with `--no-default-exclude`, extend with `--exclude`.

pub const DEFAULT_EXCLUDE: &[&str] = &[
    "*.tfstate",
    "*.tfstate.*",
    ".terraform.lock.hcl",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "Pipfile.lock",
    "Cargo.lock",
    "composer.lock",
    "Gemfile.lock",
    "go.sum",
    "*.min.js",
    "*.min.css",
];

/// Pre-compiled exclusion globs, tested against the full path AND the basename.
pub struct ExcludeSet {
    patterns: Vec<glob::Pattern>,
}

impl ExcludeSet {
    pub fn new(extra: &[String], use_default: bool) -> anyhow::Result<Self> {
        let mut patterns = Vec::new();
        for pat in extra.iter().map(String::as_str).chain(if use_default {
            DEFAULT_EXCLUDE.to_vec()
        } else {
            Vec::new()
        }) {
            patterns.push(
                glob::Pattern::new(pat)
                    .map_err(|e| anyhow::anyhow!("invalid --exclude glob '{pat}': {e}"))?,
            );
        }
        Ok(Self { patterns })
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        let basename = path.rsplit('/').next().unwrap_or(path);
        self.patterns
            .iter()
            .any(|p| p.matches(path) || p.matches(basename))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_excludes_match_path_and_basename() {
        let ex = ExcludeSet::new(&[], true).unwrap();
        assert!(ex.is_excluded("infra/terraform.tfstate"));
        assert!(ex.is_excluded("deep/dir/package-lock.json"));
        assert!(ex.is_excluded("web/app.min.js"));
        assert!(!ex.is_excluded("src/main.rs"));
    }

    #[test]
    fn extra_and_no_default() {
        let ex = ExcludeSet::new(&["*.snap".to_string()], false).unwrap();
        assert!(ex.is_excluded("tests/foo.snap"));
        assert!(!ex.is_excluded("terraform.tfstate"));
    }
}

//! Content identity of commits: `git patch-id --stable` (rebase / squash safe).
//!
//! Computed by the real `git diff-tree --stdin -p | git patch-id --stable` pipeline,
//! batched so the cost is a couple of processes per run rather than per commit.
//! Commits with no diff - empty, merge, root - have no patch-id and cache as `None`.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::gitx::{Git, Sha};

/// A stable patch-id (SHA-1 of the normalized diff).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatchId([u8; 20]);

impl PatchId {
    pub fn from_hex(hex: &str) -> Option<Self> {
        let mut bytes = [0u8; 20];
        if hex.len() != 40 {
            return None;
        }
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)?;
            let lo = (chunk[1] as char).to_digit(16)?;
            bytes[i] = (hi * 16 + lo) as u8;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Display for PatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Patch-ids for a batch of commits, via the real git pipeline.
///
/// One `diff-tree` and one `patch-id` for the whole batch, so the process cost does not
/// scale with the number of commits. Anything git prints no diff for is absent from the
/// output and stays `None`.
pub fn patch_ids(git: &Git, shas: &[Sha]) -> Result<HashMap<Sha, Option<PatchId>>> {
    let mut out: HashMap<Sha, Option<PatchId>> = shas.iter().map(|s| (*s, None)).collect();
    if shas.is_empty() {
        return Ok(out);
    }
    let input: String = shas.iter().map(|s| format!("{s}\n")).collect();
    let diff = git
        .run_with_stdin(&["diff-tree", "--stdin", "-p", "--no-color"], &input)
        .context("diff-tree pipeline failed")?;
    let ids = git
        .run_with_stdin(&["patch-id", "--stable"], &diff)
        .context("patch-id pipeline failed")?;
    for line in ids.lines() {
        // "<patch-id> <commit-id>"
        let mut parts = line.split_whitespace();
        if let (Some(pid), Some(commit)) = (parts.next(), parts.next())
            && let (Some(pid), Ok(sha)) = (PatchId::from_hex(pid), commit.parse::<Sha>())
        {
            out.insert(sha, Some(pid));
        }
    }
    Ok(out)
}

/// Shared, thread-safe patch-id cache the whole pipeline reads through.
pub struct PatchIdCache {
    map: Mutex<HashMap<Sha, Option<PatchId>>>,
    git: Git,
}

impl PatchIdCache {
    pub fn new(git: Git) -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            git,
        }
    }

    /// Compute patch-ids for any uncached shas in one batch.
    pub fn prime(&self, shas: &[Sha]) -> Result<()> {
        let missing: Vec<Sha> = {
            let map = self.map.lock().unwrap();
            shas.iter()
                .filter(|s| !map.contains_key(*s))
                .copied()
                .collect()
        };
        if missing.is_empty() {
            return Ok(());
        }
        let computed = patch_ids(&self.git, &missing)?;
        self.map.lock().unwrap().extend(computed);
        Ok(())
    }

    /// Cached patch-id for a commit (`None` = commit has no diff, or never primed).
    pub fn get(&self, sha: Sha) -> Option<PatchId> {
        self.map.lock().unwrap().get(&sha).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_id_hex_roundtrip() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        let pid = PatchId::from_hex(hex).unwrap();
        assert_eq!(pid.to_string(), hex);
        assert!(PatchId::from_hex("xyz").is_none());
        assert!(PatchId::from_hex("0123").is_none());
    }
}

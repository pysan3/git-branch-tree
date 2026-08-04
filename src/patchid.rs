//! Content identity of commits: `git patch-id --stable` (rebase / squash safe).
//!
//! Two interchangeable backends: [`Git2PatchIds`] (in-process libgit2, the default)
//! and [`SubprocessPatchIds`] (the real `git diff-tree | git patch-id` pipeline, the
//! golden reference the tests compare against). Commits with no diff (empty, merge,
//! root) have no patch-id and are cached as `None`, exactly like the original.

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

/// Computes patch-ids for batches of commits.
pub trait PatchIdSource: Send + Sync {
    fn compute(&self, shas: &[Sha]) -> Result<HashMap<Sha, Option<PatchId>>>;
}

/// The real git pipeline: `git diff-tree --stdin -p | git patch-id --stable`.
pub struct SubprocessPatchIds {
    pub git: Git,
}

impl PatchIdSource for SubprocessPatchIds {
    fn compute(&self, shas: &[Sha]) -> Result<HashMap<Sha, Option<PatchId>>> {
        let mut out: HashMap<Sha, Option<PatchId>> = shas.iter().map(|s| (*s, None)).collect();
        if shas.is_empty() {
            return Ok(out);
        }
        let input: String = shas.iter().map(|s| format!("{s}\n")).collect();
        let diff = self
            .git
            .run_with_stdin(&["diff-tree", "--stdin", "-p", "--no-color"], &input)
            .context("diff-tree pipeline failed")?;
        let ids = self
            .git
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
}

/// In-process libgit2 `git_diff_patchid` (documented to match `git patch-id --stable`).
/// A repository handle is `Send + !Sync`, so each call opens its own.
pub struct Git2PatchIds {
    pub repo_path: std::path::PathBuf,
}

impl PatchIdSource for Git2PatchIds {
    fn compute(&self, shas: &[Sha]) -> Result<HashMap<Sha, Option<PatchId>>> {
        let repo = git2::Repository::open(&self.repo_path).context("git2 open failed")?;
        let mut out = HashMap::with_capacity(shas.len());
        for &sha in shas {
            out.insert(sha, patch_id_of(&repo, sha)?);
        }
        Ok(out)
    }
}

fn patch_id_of(repo: &git2::Repository, sha: Sha) -> Result<Option<PatchId>> {
    let oid = git2::Oid::from_bytes(sha.as_bytes())?;
    let commit = repo.find_commit(oid)?;
    // Match `git diff-tree --stdin -p` semantics: merge and root commits print no
    // diff there, so they get no patch-id.
    if commit.parent_count() != 1 {
        return Ok(None);
    }
    let parent_tree = commit.parent(0)?.tree()?;
    let tree = commit.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)?;
    if diff.deltas().len() == 0 {
        return Ok(None);
    }
    let oid = diff.patchid(None)?;
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(oid.as_bytes());
    Ok(Some(PatchId(bytes)))
}

/// The backend the pipeline should use: in-process git2 when it can open the
/// repository, otherwise the subprocess pipeline. git2 avoids spawning two processes
/// per batch, which matters because every commit of every branch gets a patch-id.
pub fn patch_id_backend(repo_path: &std::path::Path, git: &Git) -> Box<dyn PatchIdSource> {
    if git2::Repository::open(repo_path).is_ok() {
        Box::new(Git2PatchIds {
            repo_path: repo_path.to_path_buf(),
        })
    } else {
        Box::new(SubprocessPatchIds { git: git.clone() })
    }
}

/// Shared, thread-safe patch-id cache the whole pipeline reads through.
pub struct PatchIdCache {
    map: Mutex<HashMap<Sha, Option<PatchId>>>,
    source: Box<dyn PatchIdSource>,
}

impl PatchIdCache {
    pub fn new(source: Box<dyn PatchIdSource>) -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            source,
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
        let computed = self.source.compute(&missing)?;
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

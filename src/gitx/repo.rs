//! Read-only repository access via gix (in-process, no subprocess per call).
//!
//! `gix::Repository` is `!Sync`, so we hold a [`gix::ThreadSafeRepository`] and each
//! caller (including every rayon worker) materialises a cheap thread-local handle.

use anyhow::{Context, Result};

/// A full object id.
pub type Sha = gix::ObjectId;

/// One entry of a raw diff (rename detection off): the file's post-image blob, or
/// `None` when the file was deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChange {
    pub path: String,
    pub blob: Option<Sha>,
}

pub struct RepoView {
    repo: gix::ThreadSafeRepository,
}

impl RepoView {
    /// Open the repository containing `dir` (discovers upward like `git`).
    pub fn discover(dir: &std::path::Path) -> Result<Self> {
        let repo = gix::discover(dir)
            .context("not inside a git repository")?
            .into_sync();
        Ok(Self { repo })
    }

    /// A thread-local handle; cheap, take one per closure.
    pub fn local(&self) -> gix::Repository {
        self.repo.to_thread_local()
    }

    /// Root of the main working tree.
    pub fn work_dir(&self) -> Result<std::path::PathBuf> {
        self.local()
            .workdir()
            .map(|p| p.to_path_buf())
            .context("repository has no working tree")
    }

    /// All local branch short names, sorted.
    pub fn local_branches(&self) -> Result<Vec<String>> {
        let repo = self.local();
        let refs = repo.references()?;
        let mut names: Vec<String> = refs
            .local_branches()?
            .filter_map(|r| r.ok())
            .map(|r| r.name().shorten().to_string())
            .collect();
        names.sort();
        Ok(names)
    }

    /// Whether a local branch with this short name exists.
    pub fn branch_exists(&self, name: &str) -> bool {
        self.local()
            .find_reference(&format!("refs/heads/{name}"))
            .is_ok()
    }

    /// Resolve a revspec (branch name, `origin/x`, sha, ...) to the commit id.
    pub fn rev_parse(&self, spec: &str) -> Result<Sha> {
        let repo = self.local();
        let id = repo
            .rev_parse_single(spec)
            .with_context(|| format!("cannot resolve '{spec}'"))?;
        Ok(id
            .object()?
            .peel_to_kind(gix::object::Kind::Commit)
            .with_context(|| format!("'{spec}' is not a commit"))?
            .id)
    }

    /// `git rev-list base..tip` — commits reachable from `tip` but not `base`,
    /// newest-first (pass the result through `.reverse()` for oldest-first).
    pub fn rev_list(&self, base: Sha, tip: Sha) -> Result<Vec<Sha>> {
        let repo = self.local();
        let walk = repo
            .rev_walk([tip])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .with_hidden([base]);
        let mut shas = Vec::new();
        for info in walk.all()? {
            shas.push(info?.id);
        }
        Ok(shas)
    }

    /// Best common ancestor, or `None` when histories are unrelated.
    pub fn merge_base(&self, a: Sha, b: Sha) -> Option<Sha> {
        let repo = self.local();
        repo.merge_base(a, b).ok().map(|id| id.detach())
    }

    /// `git merge-base --is-ancestor anc desc`.
    pub fn is_ancestor(&self, anc: Sha, desc: Sha) -> bool {
        self.merge_base(anc, desc) == Some(anc)
    }

    /// `git diff --raw --no-renames from to`: every changed path with its post-image
    /// blob id (`None` for deletions). Rename detection is intentionally off.
    pub fn raw_diff(&self, from: Sha, to: Sha) -> Result<Vec<RawChange>> {
        use gix::object::tree::diff::ChangeDetached;
        let repo = self.local();
        let from_tree = repo.find_commit(from)?.tree()?;
        let to_tree = repo.find_commit(to)?.tree()?;
        let changes = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)?;
        let mut out = Vec::with_capacity(changes.len());
        for change in changes {
            let (path, blob) = match change {
                ChangeDetached::Addition {
                    location,
                    id,
                    entry_mode,
                    ..
                } => {
                    if !entry_mode.is_blob() {
                        continue;
                    }
                    (location, Some(id))
                }
                ChangeDetached::Modification {
                    location,
                    id,
                    entry_mode,
                    ..
                } => {
                    if !entry_mode.is_blob() {
                        continue;
                    }
                    (location, Some(id))
                }
                ChangeDetached::Deletion { location, .. } => (location, None),
                ChangeDetached::Rewrite { .. } => continue, // renames are off
            };
            out.push(RawChange {
                path: path.to_string(),
                blob,
            });
        }
        Ok(out)
    }

    /// Whether `path` exists in the tree of the commit `rev` resolves to
    /// (`git cat-file -e rev:path`).
    pub fn path_in_tree(&self, rev: Sha, path: &str) -> bool {
        let repo = self.local();
        let Ok(commit) = repo.find_commit(rev) else {
            return false;
        };
        let Ok(tree) = commit.tree() else {
            return false;
        };
        matches!(tree.lookup_entry_by_path(path), Ok(Some(_)))
    }
}

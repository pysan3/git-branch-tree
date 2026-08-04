//! Bounded blame: which commit last touched a range of lines in a revision.
//!
//! Blame is bounded to the `base..rev` range so it never walks the base's own
//! (possibly huge) history for a file: lines that originate at or before the base
//! are reported against a boundary commit that we simply ignore. This keeps blame
//! cheap even for frequently-churned files.
//!
//! Subprocess `git blame` is the only backend for now: gix-blame cannot bound the
//! traversal by a commit range, and libgit2's blame is an order of magnitude slower
//! than the CLI. The trait seam exists so a crate-backed impl can slot in later.

use std::collections::HashSet;

use crate::gitx::{Git, Sha};

pub trait Blamer: Send + Sync {
    /// Commit shas that last touched lines `[lo, hi]` of `path` at `rev`, looking
    /// only at commits in `base..rev`. Failures (deleted file, binary, bad range)
    /// yield an empty set — a missing edge, never an aborted run.
    fn blame_range(&self, rev: &str, base: &str, path: &str, lo: u32, hi: u32) -> HashSet<Sha>;
}

pub struct SubprocessBlamer {
    pub git: Git,
}

impl SubprocessBlamer {
    /// Number of lines of `path` at `rev`, or `None` if absent.
    fn file_line_count(&self, rev: &str, path: &str) -> Option<u64> {
        let bytes = self
            .git
            .run_bytes(&["show", &format!("{rev}:{path}")])
            .ok()?;
        let newlines = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
        let partial_last = u64::from(!bytes.is_empty() && *bytes.last().unwrap() != b'\n');
        Some(newlines + partial_last)
    }
}

impl Blamer for SubprocessBlamer {
    fn blame_range(&self, rev: &str, base: &str, path: &str, lo: u32, hi: u32) -> HashSet<Sha> {
        let Some(nlines) = self.file_line_count(rev, path) else {
            return HashSet::new();
        };
        if nlines == 0 {
            return HashSet::new();
        }
        let lo = u64::from(lo).clamp(1, nlines);
        let hi = u64::from(hi).clamp(lo, nlines);
        let target = if !base.is_empty() && base != rev {
            format!("{base}..{rev}")
        } else {
            rev.to_string()
        };
        let range = format!("{lo},{hi}");
        let Ok(out) = self.git.run(&[
            "blame",
            "-l",
            "--porcelain",
            "-L",
            &range,
            &target,
            "--",
            path,
        ]) else {
            return HashSet::new();
        };
        // Porcelain emits `<sha> <orig-line> <final-line> [<num-lines>]` to open each
        // block, then header fields, then the tab-prefixed content line. A line that
        // originates at or before the base is attributed to a *boundary* commit,
        // flagged by a bare `boundary` header: that commit is outside base..rev, so it
        // belongs to the base rather than to any branch under analysis and must not
        // become a dependency edge.
        let mut found: HashSet<Sha> = HashSet::new();
        let mut boundary: HashSet<Sha> = HashSet::new();
        let mut current: Option<Sha> = None;
        for line in out.lines() {
            let head = line.split(' ').next().unwrap_or_default();
            if head.len() == 40
                && head.bytes().all(|b| b.is_ascii_hexdigit())
                && let Ok(sha) = head.parse::<Sha>()
            {
                found.insert(sha);
                current = Some(sha);
            } else if line == "boundary"
                && let Some(sha) = current
            {
                boundary.insert(sha);
            }
        }
        found.retain(|s| !boundary.contains(s));
        found
    }
}

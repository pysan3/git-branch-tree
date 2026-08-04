//! Diff parsing: which OLD-file lines a set of changes touches.
//!
//! The `--unified=0` hunks feed `git blame -L`, so their boundaries must be git's
//! own — an in-process diff whose heuristics shift a hunk by one line would blame a
//! neighbouring line to a different commit and flip a dependency edge. Hence the
//! diff itself is a subprocess too; if a crate-backed blamer ever lands, this and
//! the blamer must move together.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::gitx::Git;

/// A hunk range expressed in the OLD (pre-image) file: (start_line, line_count).
pub type Hunk = (u32, u32);
/// Per-file changed hunks, ordered by path.
pub type FileHunks = BTreeMap<String, Vec<Hunk>>;

/// `git diff --unified=0 --no-color from to`.
pub fn diff_unified0(git: &Git, from: &str, to: &str) -> Result<String> {
    Ok(git.run(&["diff", "--unified=0", "--no-color", from, to])?)
}

/// Parse `git diff --unified=0` output into `{file: [(old_start, old_count), ...]}`.
pub fn parse_old_side_hunks(diff: &str) -> FileHunks {
    let mut hunks: FileHunks = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            // Greedy a-side (like the original's regex): the b/ path is the suffix
            // after the LAST " b/" separator; both sides are equal with renames off.
            if let Some(idx) = rest.rfind(" b/") {
                let path = rest[idx + 3..].to_string();
                hunks.entry(path.clone()).or_default();
                current = Some(path);
            }
            continue;
        }
        let Some(ref path) = current else { continue };
        if let Some(rest) = line.strip_prefix("@@ -")
            && let Some((old, _)) = rest.split_once(" @@").map(|(head, _)| (head, ()))
            && let Some(old_range) = old.split(' ').next()
        {
            let (start, count) = match old_range.split_once(',') {
                Some((s, c)) => (s.parse::<u32>(), c.parse::<u32>()),
                None => (old_range.parse::<u32>(), Ok(1)),
            };
            if let (Ok(start), Ok(count)) = (start, count) {
                hunks
                    .get_mut(path)
                    .expect("entry created above")
                    .push((start, count));
            }
        }
    }
    hunks.retain(|_, h| !h.is_empty());
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
diff --git a/src/app.py b/src/app.py
index 111..222 100644
--- a/src/app.py
+++ b/src/app.py
@@ -10,2 +10,3 @@ def f():
-old
-old2
+new
@@ -20 +21 @@ def g():
-x
+y
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+a
+b
diff --git a/untouched.txt b/untouched.txt
old mode 100644
new mode 100755
";

    #[test]
    fn parses_old_side_ranges() {
        let hunks = parse_old_side_hunks(DIFF);
        assert_eq!(hunks.len(), 2, "mode-only file dropped: {hunks:?}");
        assert_eq!(hunks["src/app.py"], vec![(10, 2), (20, 1)]);
        assert_eq!(hunks["new.txt"], vec![(0, 0)]);
    }

    #[test]
    fn handles_paths_with_spaces() {
        let diff = "diff --git a/has space.txt b/has space.txt\n@@ -3,1 +3,1 @@\n-x\n+y\n";
        let hunks = parse_old_side_hunks(diff);
        assert_eq!(hunks["has space.txt"], vec![(3, 1)]);
    }
}

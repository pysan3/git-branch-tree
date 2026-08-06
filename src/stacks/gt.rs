//! `gt log short` - Graphite.
//!
//! Graphite has no machine-readable stack output, so this reads the drawn tree:
//!
//! ```text
//! ◯        PROJ-1/a-very-long-branch-name (needs restack)
//! │ ◯      PROJ-1/deep-nesting-check
//! │ ◯      PROJ-1/handler (needs restack)
//! │ │ ◯    PROJ-1/second-child (needs restack)
//! ◉─┴─┘    PROJ-1/api
//! ◯        main
//! ```
//!
//! Indentation grows with depth and forks collapse into `─┴─┘` runs, but the name is
//! always the first thing on the line that could be a ref, and any annotation follows
//! it. Both hold at arbitrary depth and width - checked against a real multi-root tree.
//!
//! The parser keys on nothing gt draws. Between the open-source 1.x line and 1.8.6 the
//! `▸` that used to separate the drawing from the name disappeared, and a parser that
//! recognised glyphs would have kept compiling while returning nothing - which reads
//! exactly like "you have no stack", the worst way to fail. So: drop everything before
//! the first character that could start a git ref, then take that run.
//!
//! That yields candidates. `stacks::branches` intersecting them with the repository's
//! branches is what makes it safe, and `input::from_tool` drops the trunk, which gt
//! prints as the root of the tree.

use std::borrow::Cow;

use super::{Parsing, Spec, StackTool};
use crate::suffix::SuffixPreset;

#[derive(Debug)]
pub struct GtStack;

static SPEC: Spec = Spec {
    flag: "--from-gt-stack",
    program: "gt",
    // `--stack` is not optional: without it gt lists every tracked branch in the
    // repository, so an unrelated ticket's stack would be pulled into the analysis.
    list_args: &["log", "short", "--stack"],
    probe_args: &["--version"],
    // Colour off (gt honours NO_COLOR) and pager off: gt pages when it believes it has a
    // terminal, and a paged child on a pipe is a hang rather than an error.
    env: &[
        ("NO_COLOR", "1"),
        ("FORCE_COLOR", "0"),
        ("CLICOLOR", "0"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("TERM", "dumb"),
    ],
    install: "install Graphite (https://graphite.com/docs/install-the-cli)",
    parsing: Parsing::Rendered,
    // Graphite retargets PR bases itself on `gt submit`, so leaving the crate's
    // `gh pr edit --base` default in the chain would have two tools writing the same
    // field. Teaching gt the corrected parent instead is the whole point of the flag.
    suffix: SuffixPreset {
        on_base: Some("gt track --parent {base}"),
        on_parent: Some("gt track --parent {onto}"),
    },
};

impl StackTool for GtStack {
    fn spec(&self) -> &'static Spec {
        &SPEC
    }

    fn parse(&self, stdout: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in stdout.lines() {
            // Only printed with --show-untracked, which this never passes - but if it
            // ever appears, everything below it is explicitly not part of the stack.
            if line.contains("Untracked branches") {
                break;
            }
            let line = strip_ansi(line);
            if let Some(name) = first_ref_token(&line) {
                out.push(name.to_string());
            }
        }
        out
    }
}

/// Drop ANSI escape sequences.
///
/// gt is told `NO_COLOR`, and it also drops colour on a pipe, so this should never have
/// work to do. It exists because the failure mode if it did is silent: the parameter
/// bytes of `\x1b[32m` are alphanumeric, so a coloured line would otherwise yield a
/// branch named `32m`.
fn strip_ansi(line: &str) -> Cow<'_, str> {
    if !line.contains('\u{1b}') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // A CSI sequence runs to its first final byte, in @ through ~.
            for f in chars.by_ref() {
                if ('@'..='~').contains(&f) {
                    break;
                }
            }
        }
    }
    Cow::Owned(out)
}

/// The first run of characters that could be a git ref, after whatever the renderer drew
/// in front of it. Anything after that run - ` (needs restack)`, ` (current)` - is not
/// part of the name and is never looked at.
fn first_ref_token(line: &str) -> Option<&str> {
    let start = line.find(|c: char| c.is_alphanumeric())?;
    let rest = &line[start..];
    let end = rest.find(|c: char| !is_ref_char(c)).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn is_ref_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.' | '+')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Vec<String> {
        GtStack.parse(s)
    }

    #[test]
    fn reads_the_real_1_8_6_tree() {
        // Captured verbatim from gt 1.8.6. Note the trunk is part of the drawing; it is
        // `input::from_tool` that drops it, not this.
        let out = "◯    feat-b\n│ ◉  feat-side\n◯─┘  feat-a\n◯    main\n";
        assert_eq!(parse(out), vec!["feat-b", "feat-side", "feat-a", "main"]);
    }

    #[test]
    fn reads_a_deep_wide_tree() {
        // Verbatim from gt 1.8.6: four levels of indent, a three-way fork collapsed into
        // `─┴─┴─┘`, ticket-style names with digits and slashes, and restack annotations.
        let out = "\
◯        PROJ-1/a-very-long-branch-name-that-goes-on-for-a-while (needs restack)
│ ◯      PROJ-1/deep-nesting-check
│ ◯      PROJ-1/ui
│ ◯      PROJ-1/handler (needs restack)
│ │ ◯    PROJ-1/second-child (needs restack)
│ │ │ ◯  PROJ-1/third-child (needs restack)
◉─┴─┴─┘  PROJ-1/api
◯        main
";
        assert_eq!(
            parse(out),
            vec![
                "PROJ-1/a-very-long-branch-name-that-goes-on-for-a-while",
                "PROJ-1/deep-nesting-check",
                "PROJ-1/ui",
                "PROJ-1/handler",
                "PROJ-1/second-child",
                "PROJ-1/third-child",
                "PROJ-1/api",
                "main",
            ]
        );
    }

    #[test]
    fn annotations_after_the_name_are_not_part_of_it() {
        let out = "◉  feat/b (needs restack)\n◯  feat/a (current)\n";
        assert_eq!(parse(out), vec!["feat/b", "feat/a"]);
    }

    #[test]
    fn colour_codes_do_not_become_branch_names() {
        // NO_COLOR is forced on the child, but a future gt could ignore it, and an
        // escape sequence must not turn into a name - the intersection in
        // `stacks::branches` is the backstop, this keeps it from being needed.
        let out = "\u{1b}[32m◯\u{1b}[0m  feat/a\n";
        assert_eq!(parse(out), vec!["feat/a"]);
    }

    #[test]
    fn untracked_branches_are_not_part_of_the_stack() {
        let out = "◯  feat/a\n◯  main\n\nUntracked branches:\nscratch/wip\n";
        assert_eq!(parse(out), vec!["feat/a", "main"]);
    }

    #[test]
    fn pure_drawing_and_blank_lines_name_nothing() {
        assert!(parse("│\n─┘\n\n").is_empty());
        assert!(parse("").is_empty());
    }
}

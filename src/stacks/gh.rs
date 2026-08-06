//! `gh stack view --short` - GitHub's stacked-pull-request extension.
//!
//! One branch name per line, in stack order, so the parse is a trim. The stack commands
//! are an extension rather than part of `gh` itself, so the probe is `gh stack --help`:
//! a working `gh` without the extension would otherwise pass preflight and then fail on
//! the one command that matters.
//!
//! `gh stack view` shows the *current* stack and takes no stack selector, so this only
//! works from a branch that is in one.

use super::{Spec, StackTool};

#[derive(Debug)]
pub struct GhStack;

static SPEC: Spec = Spec {
    flag: "--from-gh-stack",
    program: "gh",
    list_args: &["stack", "view", "--short"],
    probe_args: &["stack", "--help"],
    env: &[
        ("NO_COLOR", "1"),
        ("GH_PAGER", "cat"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
    ],
    install: "install it with `gh extension install github/gh-stack`",
};

impl StackTool for GhStack {
    fn spec(&self) -> &'static Spec {
        &SPEC
    }

    fn parse(&self, stdout: &str) -> Vec<String> {
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Vec<String> {
        GhStack.parse(s)
    }

    #[test]
    fn one_branch_per_line_in_the_order_given() {
        assert_eq!(
            parse("feat/api\nfeat/handler\nfeat/ui\n"),
            vec!["feat/api", "feat/handler", "feat/ui"]
        );
    }

    #[test]
    fn blank_lines_and_padding_are_ignored() {
        // A trailing newline and any indentation must not become a branch named "".
        assert_eq!(
            parse("\n  feat/api  \n\nfeat/ui\n\n"),
            vec!["feat/api", "feat/ui"]
        );
    }

    #[test]
    fn empty_output_names_nothing() {
        // `stacks::branches` turns this into an error; the parser itself just says none.
        assert!(parse("").is_empty());
        assert!(parse("\n \n").is_empty());
    }
}

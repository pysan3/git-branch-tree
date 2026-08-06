//! Reading the branch list from whatever already manages the stack.
//!
//! Everything the crate knows about an external tool - what to run, what environment it
//! needs, how to probe it, how to read its output, what to say when it is missing - lives
//! in one file under this module. Nothing outside [`TOOLS`] enumerates them, so adding or
//! dropping a tool is that file plus one line here.
//!
//! Note what is deliberately *not* read: the parent relationships these tools record.
//! Their declared structure is the hypothesis this crate exists to test, so taking edges
//! from it would be circular. Only the branch set comes from them.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};

use crate::gitx::Git;
use crate::util::{note, warn};

pub mod gh;

/// Everything about a tool that is data rather than behaviour.
///
/// Keeping it data is what makes a tool file mechanical to write: a `static SPEC` and a
/// `parse`, with no room to silently omit an answer some other module needed.
pub struct Spec {
    /// The flag that selects it. Also its registry key, and how every message refers to
    /// it - the user typed a flag, not a tool.
    pub flag: &'static str,
    /// Executable on PATH.
    pub program: &'static str,
    /// Arguments that print the stack.
    pub list_args: &'static [&'static str],
    /// Arguments that prove the tool is usable, run by preflight. Not always
    /// `--version`: `gh stack` is an extension, so a working `gh` proves nothing about
    /// it.
    pub probe_args: &'static [&'static str],
    /// Environment forced on the child: pager and colour off, which a parser must not
    /// see. Applied to the probe too, so that cannot hang on a pager either.
    pub env: &'static [(&'static str, &'static str)],
    /// The remedy half of the preflight message; the caller appends ", or drop <flag>".
    pub install: &'static str,
}

pub trait StackTool: Sync + std::fmt::Debug {
    fn spec(&self) -> &'static Spec;

    /// The branch names in the tool's output, in the order it listed them.
    ///
    /// Pure: no IO, no repository access, and no checking against reality - [`branches`]
    /// does that once, for every tool. Being a total function over a `&str` is what lets
    /// it be tested against pasted real output with nothing installed.
    fn parse(&self, stdout: &str) -> Vec<String>;
}

/// The tools this build knows about. Nothing else enumerates them.
pub const TOOLS: &[&'static dyn StackTool] = &[&gh::GhStack];

/// The tool a flag selects. `None` means the CLI and this registry disagree, which is a
/// bug here rather than anything the user typed.
pub fn by_flag(flag: &str) -> Option<&'static dyn StackTool> {
    TOOLS.iter().copied().find(|t| t.spec().flag == flag)
}

/// Run `tool` and return the local branches it named, in its own order.
///
/// Intersecting with the repository's own branches is load-bearing rather than tidy-up:
/// it is what stops a rendering change in someone else's CLI from turning into a branch
/// name that does not exist.
pub fn branches(tool: &dyn StackTool, git: &Git, locals: &[String]) -> Result<Vec<String>> {
    let spec = tool.spec();
    let cmdline = format!("{} {}", spec.program, spec.list_args.join(" "));
    let out = git
        .tool(spec.program, spec.list_args, spec.env)
        .with_context(|| format!("cannot list the stack for {} (`{cmdline}`)", spec.flag))?;

    let named = tool.parse(&out);
    if named.is_empty() {
        bail!("{}: `{cmdline}` named no branches", spec.flag);
    }

    let known: HashSet<&str> = locals.iter().map(String::as_str).collect();
    let (kept, dropped): (Vec<String>, Vec<String>) =
        named.into_iter().partition(|n| known.contains(n.as_str()));

    if kept.is_empty() {
        // Distinct from "you have no stack" on purpose: this says the parse went wrong,
        // not that there was nothing to find.
        bail!(
            "{}: none of the {} name(s) from `{cmdline}` is a local branch ({}); \
             its output may not be in the form this build expects",
            spec.flag,
            dropped.len(),
            dropped.join(", ")
        );
    }
    if !dropped.is_empty() {
        // Not fatal: a branch deleted locally but still in the stack should not sink the
        // whole run.
        warn(&format!(
            "{}: not a local branch, skipped: {}",
            spec.flag,
            dropped.join(", ")
        ));
    }
    // Silence here would be indistinguishable from the flag never running.
    note(&format!(
        "{}: {} branch(es) from `{cmdline}`",
        spec.flag,
        kept.len()
    ));
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_tool_is_reachable_by_its_flag() {
        // The guard that keeps TOOLS and the clap declarations from drifting apart.
        for tool in TOOLS {
            let flag = tool.spec().flag;
            assert!(by_flag(flag).is_some(), "{flag} is not reachable");
            assert!(flag.starts_with("--"), "{flag} is not a long flag");
        }
    }
}

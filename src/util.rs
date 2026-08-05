//! Output conventions shared by the whole pipeline.
//!
//! stdout carries the report only (header, trees, rebase block); everything
//! operational goes to stderr as `# `-prefixed notes so the report stays clean
//! and copy-pasteable.

/// Print an operational note to stderr.
pub fn note(msg: &str) {
    eprintln!("# {msg}");
}

/// Print a warning to stderr.
pub fn warn(msg: &str) {
    eprintln!("# warning: {msg}");
}

/// Shorten an object id for display in generated commands.
pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

/// Quote a ref for safe inclusion in the emitted shell commands.
///
/// git permits `;`, `$(...)`, backticks, `|` and friends in ref names - only whitespace
/// and a couple of glob characters are refused - so a branch fetched from an untrusted
/// fork could otherwise smuggle commands into the copy-pasteable rebase block, which
/// runs the moment it is pasted. Quoting is [`shlex`]'s job, not ours; it leaves
/// anything that needs no quoting untouched, so ordinary output stays readable.
///
/// `shlex` only fails on an interior NUL, which git already forbids in a ref name; the
/// fallback drops it rather than risking an unquoted value reaching the shell.
pub fn shell_quote(s: &str) -> std::borrow::Cow<'_, str> {
    shlex::try_quote(s).unwrap_or_else(|_| s.replace('\0', "").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_truncates_to_ten() {
        assert_eq!(short("0123456789abcdef"), "0123456789");
        assert_eq!(short("abc"), "abc");
    }

    /// Feed one quoted word through a real shell and return the argument it produced.
    /// Unix only - `shlex` quotes for POSIX shells, which is what the emitted block
    /// targets, so a round trip has to be checked against one.
    #[cfg(unix)]
    fn through_shell(quoted: &str) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {quoted}"))
            .output()
            .expect("spawn sh");
        assert!(out.status.success(), "shell rejected: {quoted}");
        String::from_utf8(out.stdout).expect("utf8")
    }

    #[test]
    fn ordinary_refs_pass_through_unquoted() {
        // Keeps the emitted block readable, and identical to what the original printed.
        for name in ["main", "feat/x", "PROJ-412/some_thing.v2", "origin/master"] {
            assert_eq!(shell_quote(name), name, "{name} should not be quoted");
        }
    }

    #[test]
    #[cfg(unix)]
    fn dangerous_refs_survive_a_shell_round_trip_without_executing() {
        // The real property: whatever git allows in a ref name must reach the command as
        // one literal argument, never as shell syntax. Verified against a real shell so
        // it does not depend on any particular quoting style.
        for name in [
            "main",
            "feat/x",
            "feat/x;id",
            "feat/$(id)",
            "feat/`id`",
            "feat/a|b",
            "feat/a&b",
            "feat/a>b",
            "feat/a<b",
            "feat/a#b",
            "feat/a!b",
            "feat/a(b)",
            "it's",
            "say-\"hi\"",
            "a+b=c,d:e",
            "日本語/機能",
        ] {
            let quoted = shell_quote(name);
            assert_eq!(through_shell(&quoted), name, "round trip for {name}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_command_substitution_does_not_run() {
        // If quoting failed, the shell would run `id` and print something else entirely.
        let quoted = shell_quote("feat/$(id)");
        assert_eq!(through_shell(&quoted), "feat/$(id)");
    }
}

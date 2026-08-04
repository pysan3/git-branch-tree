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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_truncates_to_ten() {
        assert_eq!(short("0123456789abcdef"), "0123456789");
        assert_eq!(short("abc"), "abc");
    }
}

//! Subprocess façade for the operations no crate covers faithfully: bounded blame,
//! worktrees, rebase --onto, fetch/pull, and the `gh` CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A failed subprocess, rendered like the Python original:
/// `git <args> failed (<code>): <stderr>`.
#[derive(Debug, thiserror::Error)]
#[error("{program} {args} failed ({status}):\n{stderr}")]
pub struct CmdError {
    pub program: String,
    pub args: String,
    pub status: i32,
    pub stderr: String,
}

/// Runs `git` (and `gh`) with a fixed working directory, so the whole pipeline is
/// independent of the process cwd (and trivially testable against tempdir repos).
#[derive(Debug, Clone)]
pub struct Git {
    dir: PathBuf,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Run `git <args>` in the repo directory and return its trimmed stdout.
    pub fn run(&self, args: &[&str]) -> Result<String, CmdError> {
        self.run_in(&self.dir, args)
    }

    /// Run `git <args>` in an arbitrary directory (used for worktrees).
    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Result<String, CmdError> {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map_err(|e| spawn_error("git", args, &e))?;
        collect("git", args, &out.status, out.stdout, out.stderr)
    }

    /// Run `git <args>`, returning only whether it exited 0 (output discarded).
    pub fn ok(&self, args: &[&str]) -> bool {
        self.ok_in(&self.dir, args)
    }

    /// [`Self::ok`] in an arbitrary directory.
    pub fn ok_in(&self, dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Run `git <args>` and return raw, untrimmed stdout bytes (file content reads,
    /// where trailing newlines are significant).
    pub fn run_bytes(&self, args: &[&str]) -> Result<Vec<u8>, CmdError> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .output()
            .map_err(|e| spawn_error("git", args, &e))?;
        if !out.status.success() {
            return Err(CmdError {
                program: "git".into(),
                args: args.join(" "),
                status: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(out.stdout)
    }

    /// Run `git <args>` feeding `input` on stdin (the batched patch-id pipeline).
    pub fn run_with_stdin(&self, args: &[&str], input: &str) -> Result<String, CmdError> {
        use std::io::Write;
        let mut child = Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spawn_error("git", args, &e))?;
        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(input.as_bytes())
            .map_err(|e| spawn_error("git", args, &e))?;
        let out = child
            .wait_with_output()
            .map_err(|e| spawn_error("git", args, &e))?;
        collect("git", args, &out.status, out.stdout, out.stderr)
    }

    /// Run `gh <args>` (GitHub CLI) and return its trimmed stdout.
    pub fn gh(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = Command::new("gh")
            .args(args)
            .current_dir(&self.dir)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow::anyhow!("gh (GitHub CLI) not found; install it or omit --auto-merged")
                } else {
                    anyhow::Error::from(spawn_error("gh", args, &e))
                }
            })?;
        Ok(collect("gh", args, &out.status, out.stdout, out.stderr)?)
    }
}

fn spawn_error(program: &str, args: &[&str], err: &std::io::Error) -> CmdError {
    CmdError {
        program: program.to_string(),
        args: args.join(" "),
        status: -1,
        stderr: err.to_string(),
    }
}

fn collect(
    program: &str,
    args: &[&str],
    status: &std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> Result<String, CmdError> {
    if !status.success() {
        return Err(CmdError {
            program: program.to_string(),
            args: args.join(" "),
            status: status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        });
    }
    let text = String::from_utf8_lossy(&stdout).into_owned();
    Ok(text.trim_end_matches('\n').to_string())
}

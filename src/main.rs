// SIGINT keeps its default disposition: the process dies of the signal and the shell
// reports exit 130, and child git/test processes in the same group receive it too.
use clap::Parser;

use git_branch_tree::app;
use git_branch_tree::cli::Cli;

fn main() {
    if let Err(err) = app::run(Cli::parse()) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

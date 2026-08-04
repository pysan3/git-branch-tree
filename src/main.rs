// SIGINT keeps its default disposition: the process dies of the signal and the shell
// reports exit 130, and child git/test processes in the same group receive it too.
fn main() {
    if let Err(err) = git_branch_tree::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

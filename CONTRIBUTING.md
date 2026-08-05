# Contributing

Thanks for taking a look.

## The PR title must be a conventional commit

`main` accepts squash merges only, and the squash commit takes its subject from the
**pull request title** — so the PR title is the one message that lands on `main`. Your
individual commits are collapsed and their messages do not survive the merge; write them
for reviewers, not for the changelog.

That title is what [release-plz](https://release-plz.dev) reads to work out the next
version and the changelog entry, so
[Conventional Commits](https://www.conventionalcommits.org) format is load-bearing here
rather than a style preference:

```
feat: add --on-parent for stacked PR retargeting
fix: quote refs in the emitted rebase block
docs: explain the squash-merge skip point
test: cover the diamond case
ci: pin the MSRV job to rust-version
chore: bump gix
```

Which prefix you choose decides whether a release happens at all: `feat:` bumps the
minor version and `fix:` the patch, while `docs:`, `test:`, `ci:` and `chore:` change
nothing that gets published. Use `feat!:`, or a `BREAKING CHANGE:` trailer in the PR
body, for anything that alters the emitted commands or the CLI surface — the rebase
block gets pasted into a shell, so a change there is a change to what people run.

The PR body becomes the commit body, which is where a `BREAKING CHANGE:` trailer needs
to go to be picked up.

## Before opening a PR

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

CI runs exactly these, plus an MSRV build, `cargo-deny` and a spellcheck. `main` is
protected: a pull request is required, and the single `ci` check has to pass before it
can be merged.

## Tests

Integration tests drive **real git** in temporary directories — there are no mocks, and
the fixtures are milliseconds each. If you touch the dependency engine or a renderer, add
a fixture that shows the shape you fixed; several of the trickier cases (the diamond, the
squash-merge skip point) exist because an invented expectation turned out to be wrong.

Output is pinned byte for byte on purpose — the rebase block gets pasted into a shell, so
a change to it is a change to what people run. If a change alters the report, that is a
deliberate decision to state in the commit message, not incidental drift.

## Scope

The tool derives structure from content and holds no state. Proposals that require
recording metadata, rewriting commit messages, or maintaining a side-channel of stack
relationships are a different tool — that is what Graphite and ghstack already do well.

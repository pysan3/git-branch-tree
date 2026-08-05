# Contributing

Thanks for taking a look.

## Conventional commits are required

Releases are automated by [release-plz](https://release-plz.dev), which derives the next
version and the changelog from commit messages. A commit that does not follow
[Conventional Commits](https://www.conventionalcommits.org) will land in neither, so the
format is load-bearing rather than a style preference.

```
feat: add --on-parent for stacked PR retargeting
fix: quote refs in the emitted rebase block
docs: explain the squash-merge skip point
test: cover the diamond case
ci: pin the MSRV job to rust-version
chore: bump gix
```

Use `feat!:` (or a `BREAKING CHANGE:` trailer) for anything that changes the emitted
commands or the CLI surface — the rebase block gets pasted into a shell, so a change there
is a change to what people run.

## Before opening a PR

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

CI runs exactly these, plus an MSRV build, `cargo-deny` and a spellcheck.

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

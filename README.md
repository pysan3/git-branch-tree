# git-branch-tree

**Write in a stack, review as a tree.**

For [stacked pull requests](https://docs.github.com/en/pull-requests/how-tos/stacked-pull-requests):
finds which branches in a stack actually depend on each other — from code content, not
branch ancestry — and rebases the rest onto the base so they can be reviewed and merged
independently. Survives rebases and squash-merges.

[![CI](https://github.com/pysan3/git-branch-tree/actions/workflows/ci.yml/badge.svg)](https://github.com/pysan3/git-branch-tree/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/git-branch-tree.svg)](https://crates.io/crates/git-branch-tree)
[![downloads](https://img.shields.io/crates/d/git-branch-tree.svg)](https://crates.io/crates/git-branch-tree)
[![license](https://img.shields.io/crates/l/git-branch-tree.svg)](./LICENSE)
![msrv](https://img.shields.io/badge/rustc-1.88+-blue.svg)

---

## The problem

A git branch has one parent, so stacking work means piling each new branch onto the
last one:

```
main → A → B → C → D
```

That chain is the order you wrote things in, not what depends on what. If only `B`
really needs `A`, `C` and `D` still can't be reviewed until `A` and `B` land first.

## Demo

```console
$ git-branch-tree --format ascii A
# base: main
# branches (4): A, B, C, D

main
├─ A
│  └─ B
├─ C
└─ D

true \
&& git rebase --onto A 4733cc0148 B && git checkout B && git push --force-with-lease && gh pr edit B --base A \
&& git rebase --onto main 5772663190 C && git checkout C && git push --force-with-lease && review \
&& git rebase --onto main b9e13b070d D && git checkout D && git push --force-with-lease && review \
&& true
```

`git-branch-tree` found the one real dependency (`B` on `A`, because `B` edits a line `A`
wrote) and flattened the rest. Paste the block: `C` and `D` become independent PRs against
`main`, reviewable right away, while `B` stays stacked on `A` with its PR base retargeted
to match.

## How it compares

Every other stacking tool asks you to **declare** the structure and then keeps it honest
for you. `git-branch-tree` **derives** the structure you never declared — which is why it
is the only one that can tell you a branch never needed its parent in the first place.

|  | git-branch-tree | [GitHub Stacked PRs][d-ghs] | [Graphite][d-gt] | [ghstack][d-ez] |
| --- | --- | --- | --- | --- |
| Structure comes from | your diffs (patch-ids + blame) | the base branch you pick per PR | the branch you ran `gt create` on | your commit order |
| Shape it can express | tree, multi-parent | [linear chain][d-ghs] | [tree][d-gtnav] | linear chain |
| Finds branches that never needed their parent | **yes** | no | no | no |
| Keeps the stack rebased as you work | no | [yes][d-ghcli] | [yes][d-gtre] | yes |
| Opens and retargets the PRs | prints commands | yes | yes | yes |
| Checks a branch really builds on the base | `--test` | no | no | no |
| Stack state it keeps | none | local tracking + GitHub | Graphite metadata | `gh/<user>/N/*` branches |
| Can print its stack for scripts | — | [`gh stack view --json`][d-ghcli], REST API | `gt log short`, text only | — |
| Needs an account or service | no | GitHub | Graphite | no |

[d-ghs]: https://docs.github.com/en/pull-requests/get-started/about-stacked-prs
[d-ghcli]: https://docs.github.com/en/pull-requests/reference/stacked-prs-cli-commands
[d-gt]: https://graphite.com/docs/cli-overview
[d-gtnav]: https://graphite.com/docs/navigate-stack
[d-gtre]: https://graphite.com/docs/restack-branches
[d-ez]: https://github.com/ezyang/ghstack

**[GitHub Stacked PRs](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs)**
(`gh stack`, public preview since July 2026) — native, no third party, and reviewers see
the stack in the GitHub UI. But a stack is strictly a chain: *"Each subsequent pull
request targets the branch of the pull request below it."* You place each change by hand
— *"Create a new branch when you start a different concern that depends on what you've
built so far"* — and nothing ever re-examines that choice. Restructuring is
[`gh stack modify`](https://docs.github.com/en/pull-requests/reference/stacked-prs-cli-commands),
an interactive editor you drive yourself.

**[Graphite](https://graphite.com/docs/cli-overview)** — the most capable of the four:
stacks are real trees ([`gt up` prompts you when a branch has several children][d-gtnav]),
`gt restack` cascades a parent's changes down, and it comes with a review UI and merge
queue. Still, the tree is the one you built: adopting a branch git created means
[telling it the parent yourself][d-gttrack] (`gt track`), and its automatic mode
*"chooses the nearest ancestor"* — ancestry again, the thing that was wrong to begin
with. Requires a Graphite account.

**[ghstack](https://github.com/ezyang/ghstack)** — the strictest: *"Every commit in your
local commit stack gets submitted into a separate pull request."* No dependency analysis
at all, N commits always become N chained PRs, and it warns that *"You will NOT be able
to merge these commits using the normal GitHub UI."*

[d-gttrack]: https://graphite.com/docs/track-branches

### And the honest case against this one

It is a one-shot analyser, not a workflow. It will not create your PRs, will not keep
your stack rebased while you work, and gives reviewers no stack UI — so if you want
those, run one of the above and use `git-branch-tree` for the question they cannot
answer. Its edges are heuristics over text, so a dependency with no textual overlap
needs `--test` to catch. And un-flattening rewrites branches, which means force-pushing
anything already under review.

## Install

```sh
cargo install git-branch-tree          # from source
cargo binstall git-branch-tree         # prebuilt binary, no compile
```

Prebuilt archives for macOS, Linux and Windows are attached to every
[release](https://github.com/pysan3/git-branch-tree/releases).

Needs `git` on `PATH`. `--auto-merged` additionally needs the
[`gh` CLI](https://cli.github.com) and network access.

## Quickstart

```sh
# one branch: it, plus every local branch stacked on top of it
git-branch-tree PROJ-123/api

# several branches: exactly those
git-branch-tree feat/a feat/b feat/c

# every branch sharing a ticket prefix
git-branch-tree --prefix PROJ-123

# ...or the whole ticket family, by leading-letter group
git-branch-tree --alpha --prefix PROJ-123

# already using a stacking tool? take the branch list from it
git-branch-tree --from-gh-stack
git-branch-tree --from-gt-stack
```

Both read the current stack — `gh stack view --short` and `gt log short --stack` — and
take its **branch set only**, never the order or bases it declares. Those are exactly the
hypothesis this tool exists to test, so trusting them would be circular; the graph is
still derived from your code.

`--from-gt-stack` additionally emits `gt track --parent` instead of the default
`gh pr edit --base`, so Graphite learns the corrected tree. Graphite retargets PR bases
itself on `gt submit`, so leaving `gh pr edit` in the chain would put two tools on the
same field. Pass `--on-base`/`--on-parent` explicitly to override either side.

Output is Mermaid by default (paste it into a PR description and GitHub renders the
graph); `--format ascii` prints the tree above, `--format both` prints both.

## How it works

Three signals decide whether `B` depends on `A`, none of which is ancestry:

1. **Patch-ids isolate each branch's own work.** `git patch-id` hashes a commit's *diff*,
   not its identity, so a change keeps the same id through a rebase, a cherry-pick or a
   squash-merge. A branch's own commits are the ones its chain-upstream does not already
   carry — so "what did this branch add" is a set difference over content, and it still
   holds after the stack is rebased.

2. **Bounded blame finds who wrote the lines you edited.** For each line a branch
   changes, `git blame` says which commit last touched it, and therefore which branch
   introduced it. If that traces back to another branch in the set, it is a real
   dependency; if it traces back to the base, it is not. Blame is bounded to
   `base..<parent>`, so a churned file never drags the base's whole history in.

   This is why *same file* is not *same code*: two branches editing different lines of
   one file stay independent.

3. **Content containment catches the inverse.** A branch carrying another's identical new
   files, with no ancestry link between them, genuinely cannot land first.

The resulting graph is transitively reduced, so each branch hangs off its *nearest*
dependencies only. A branch may have several parents; Mermaid draws that directly, and
the ASCII tree annotates it as `(also depends on: ...)`.

### What it can't see

A dependency with no textual overlap — `D` calls a function `B` defines, and nothing
conflicts. That is what `--test` is for: it performs the exact rebase it would emit, in a
throwaway worktree, then runs your command there. If the branch cannot actually build or
pass tests on the base, it is dropped from the block and listed with the reason, along
with anything stacked on it.

```sh
git-branch-tree --prefix PROJ-123 --test 'cargo test --quiet'
```

## Squash-merges

When a branch squash-merges, its code is in `main` under a hash that never existed on
your branch. A naive rebase would replay those commits and conflict with the squashed
version. Tell the tool what landed and it finds the skip point by content:

```sh
git-branch-tree --prefix PROJ-123 --merged PROJ-123/api
git-branch-tree --prefix PROJ-123 --auto-merged   # ask GitHub instead
```

Merged branches drop out of the tree — they *are* the base now — and anything that
depended only on them is repointed at the base.

## Flags

| Flag | Meaning |
| --- | --- |
| `<branch>...` | One branch (plus everything stacked on it), or several exact branches |
| `--prefix <P>...` | Every local branch matching any prefix |
| `--alpha` | With `--prefix`, match by leading-letter group (`PROJ-123` → every `PROJ-*`) |
| `--from-gh-stack` | Take the branch list from the current `gh stack` (its branch set only, never its edges) |
| `--from-gt-stack` | The same for the current Graphite stack; also defaults the suffixes to `gt track --parent` |
| `--base <ref>` | Base branch (default: auto-detect via `origin/HEAD`, then `main`/`master`) |
| `--merged <B>...` | Branches already squash-merged into the base (space- or comma-separated) |
| `--auto-merged` | Also treat as merged any branch whose GitHub PR has merged (needs `gh`) |
| `--ancestry` | Trust the git graph instead of the content heuristics |
| `--format <F>` | `ascii`, `mermaid` (default) or `both` |
| `-j, --jobs <N>` | Parallel git workers (default: 2× cores, capped at 32) |
| `--exclude <G>...` | Extra path globs to ignore when detecting dependencies |
| `--no-default-exclude` | Keep lockfiles and generated files, which are skipped by default |
| `--skip-ambiguous` | Omit branches needing more than one unmerged parent; list them instead |
| `--test <CMD>` | Verify each base-targeted branch really works on the base |
| `--test-jobs <N>` | Workers for `--test` (default: `1`) — each holds a full worktree |
| `--test-patch <P>` | `git apply` this patch in each worktree before `--test` |
| `--on-base <CMD>` | Command appended after each branch that lands on the base |
| `--on-parent <CMD>` | Command appended after each branch that lands on a parent |
| `--no-fetch` | Do not refresh the base from origin first |
| `--no-rebase` | Print the tree only |

### Tailoring the emitted commands

Each rebase is followed by whatever finishes the job in your setup. The defaults assume a
`review` command and the `gh` CLI, and both are templates you can replace — placeholders
are `{branch}`, `{onto}`, `{base}` and `{up}`:

```sh
git-branch-tree --prefix PROJ-123 \
  --on-base   'gh pr create --base {base} --head {branch} --fill' \
  --on-parent 'gh pr edit {branch} --base {onto}'
```

Repeat a flag to chain several commands; pass an empty value to append nothing.

### Handing the result back to `gt` or `gh stack`

`--from-gt-stack` already does this for Graphite: it emits
[`gt track --parent`](https://graphite.com/docs/track-branches) for every branch, so `gt`
learns the corrected tree as the block runs. That works because the chain checks each
branch out before its suffix, and emits branches dependencies-first — a parent is always
tracked before the child naming it, which is what `gt track --parent` requires. Use the
templates only to change it:

```sh
git-branch-tree --from-gt-stack --on-base 'gt submit --no-interactive'
```

`gh stack` has no per-branch equivalent — `gh stack modify` is an interactive editor — so
there is nothing to emit. Instead, every root-to-leaf path in the printed tree is one
stack: `gh stack init A B` adopts the `A → B` chain, and the branches that got flattened
stay ordinary single PRs.

## The loop

```sh
git-branch-tree --prefix PROJ-123 --auto-merged --test 'make test'   # look
<paste the block>                                                   # flatten
# ...reviews land, some branches merge...
git-branch-tree --prefix PROJ-123 --auto-merged                      # look again
```

Every run recomputes from scratch against a freshly fetched base, so it stays correct as
branches merge and the remaining ones shift. There is no state to maintain and nothing to
keep in sync — which is the whole point of deriving the structure from content instead of
recording it.

## Notes

- Before analysing, the base is refreshed from origin so every diff is computed against
  current code. This is worktree-aware: git refuses to move a ref that is checked out, so
  a base checked out elsewhere is pulled there. Skip with `--no-fetch`.
- The rebase block is one `&&` chain bookended with `true`, so pasting it runs everything
  and stops at the first failure. Refs are shell-quoted where needed — git permits `;` and
  `$(...)` in branch names.
- stdout carries the report only; operational notes go to stderr prefixed with `# `. So
  `git-branch-tree ... > plan.sh` captures just the report.

## License

Mozilla Public License 2.0 © 2026- pysan3

MPL-2.0 is file-scoped: you can combine this with code under other licences, including
proprietary code, and only changes to *these* files have to be shared back.

Versions up to and including 0.2.x were published under MIT and stay that way — crates.io
releases are immutable.

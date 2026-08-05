# git-branch-tree

**Write in a stack, review as a tree.**

Compute the real dependency DAG of stacked branches from *content* — not ancestry — and
get the exact rebase commands to un-flatten them. Survives rebases and squash-merges.

[![CI](https://github.com/pysan3/git-branch-tree/actions/workflows/ci.yml/badge.svg)](https://github.com/pysan3/git-branch-tree/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/git-branch-tree.svg)](https://crates.io/crates/git-branch-tree)
[![downloads](https://img.shields.io/crates/d/git-branch-tree.svg)](https://crates.io/crates/git-branch-tree)
[![license](https://img.shields.io/crates/l/git-branch-tree.svg)](./LICENSE)
![msrv](https://img.shields.io/badge/rustc-1.88+-blue.svg)

---

## The problem

You are working on four things at once. Naturally, you stack them:

```
main → A → B → C → D
```

You had to. A git branch has exactly **one** parent, so the only way to keep working is
to pile the next branch on the last one. But that line is a lie about your work. Say
`A` adds an auth argument to a handler, `B` adds logging to that same handler, `C` tweaks
some CSS, and `D` fixes a typo in the README. Only `B` actually needs `A`.

Reviewers pay for that lie. `D` cannot be reviewed or merged until `A`, `B` and `C` land,
even though it depends on none of them. One slow review blocks three ready changes.

The obvious fix — "just rebase the independent ones onto main" — needs an answer to a
question git will not give you: **which of these branches actually depend on each other?**
Ancestry cannot tell you; it only records where you happened to branch from. And the
moment something squash-merges, the landed commits get brand-new hashes, so even
comparing commits stops working.

`git-branch-tree` works it out from the code itself, after the fact:

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

It found the one real dependency — `B` on `A`, because `B` edits the line `A` wrote — and
flattened the other two. Paste the block and `C` and `D` become independent PRs against
`main`, reviewable immediately, while `B` stays stacked on `A` with its PR base retargeted
to match.

> This is deliberately the *retroactive* counterpart to tools like
> [Graphite](https://graphite.dev) or [ghstack](https://github.com/ezyang/ghstack), which
> ask you to declare the structure up front and maintain it as you go. Here you keep
> working the way git pushes you to — one branch on the last — and recover the real shape
> afterwards, from the content.

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
```

Output is Mermaid by default (paste it into a PR description and GitHub renders the
graph); `--format ascii` prints the tree above, `--format both` prints both.

## How it decides that B depends on A

Three signals, none of which is ancestry:

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

### What it cannot see

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

MIT © 2026- pysan3

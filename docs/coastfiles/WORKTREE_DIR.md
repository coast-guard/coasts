# Worktree Directories

The `worktree_dir` field in `[coast]` controls where git worktrees live. Coast uses git worktrees to give each instance its own copy of the codebase on a different branch, without duplicating the full repo.

## Syntax

`worktree_dir` accepts a single string or an array of strings:

```toml
# Single directory (default)
worktree_dir = ".worktrees"

# Multiple directories
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

When omitted, defaults to `".worktrees"`.

## Path types

### Relative paths

Paths that don't start with `~/` or `/` are resolved relative to the project root. These are the most common and require no special handling — they're inside the project directory and automatically available inside the Coast container via the standard `/host-project` bind mount.

```toml
worktree_dir = ".worktrees"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### Tilde paths (external)

Paths starting with `~/` are expanded to the user's home directory and treated as **external** worktree directories. Coast adds a separate bind mount so the container can access them.

```toml
worktree_dir = ["~/.codex/worktrees", ".worktrees"]
```

This is how you integrate with tools that create worktrees outside your project root, such as OpenAI Codex (which always creates worktrees at `$CODEX_HOME/worktrees`).

### Absolute paths (external)

Paths starting with `/` are also treated as external and get their own bind mount.

```toml
worktree_dir = ["/shared/worktrees", ".worktrees"]
```

## How external directories work

When Coast encounters an external worktree directory (tilde or absolute path), three things happen:

1. **Container bind mount** — At container creation time (`coast run`), the resolved host path is bind-mounted into the container at `/host-external-wt/{index}`, where `{index}` is the position in the `worktree_dir` array. This makes the external files accessible inside the container.

2. **Project filtering** — External directories may contain worktrees for multiple projects. Coast uses `git worktree list --porcelain` (which is inherently scoped to the current repository) to discover only the worktrees that belong to this project. The git watcher also verifies ownership by reading each worktree's `.git` file and checking that its `gitdir:` pointer resolves back to the current repo.

3. **Workspace remount** — When you `coast assign` to an external worktree, Coast remounts `/workspace` from the external bind mount path instead of the usual `/host-project/{dir}/{name}`.

## Naming of external worktrees

External worktrees with a branch checked out appear by their branch name, the same as local worktrees.

External worktrees on a **detached HEAD** (common with Codex) appear using their relative path within the external directory. For example, a Codex worktree at `~/.codex/worktrees/a0db/coastguard-platform` appears as `a0db/coastguard-platform` in the UI and CLI.

## `default_worktree_dir`

Controls which directory is used when Coast creates a **new** worktree (e.g., when you assign a branch that doesn't have an existing worktree). Defaults to the first entry in `worktree_dir`.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

External directories are never used for creating new worktrees — Coast always creates worktrees in a local (relative) directory. The `default_worktree_dir` field is only needed when you want to override the default (first entry).

## Examples

### Codex integration

OpenAI Codex creates worktrees at `~/.codex/worktrees/{hash}/{project-name}`. To make these visible and assignable in Coast:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

After adding this, Codex's worktrees show up in the checkout modal and `coast ls` output. You can assign a Coast instance to a Codex worktree to run its code in a full development environment.

Note: the container must be recreated (`coast run`) after adding an external directory for the bind mount to take effect. Restarting an existing instance is not sufficient.

### Claude Code integration

Claude Code creates worktrees inside the project at `.claude/worktrees/`. Since this is a relative path (inside the project root), it works like any other local worktree directory — no external mount needed:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### All three together

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

## Live Coastfile reading

Changes to `worktree_dir` in your Coastfile take effect immediately for worktree **listing** (the API and git watcher read the live Coastfile from disk, not just the cached build artifact). However, external **bind mounts** are only created at container creation time, so you need to recreate the instance for a newly added external directory to be mountable.

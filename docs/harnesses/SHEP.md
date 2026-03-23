# Shep

## Quick setup

Requires the [Coast CLI](../GETTING_STARTED.md). Copy this prompt into your
agent's chat to set up Coasts automatically:

```prompt-copy
shep_setup_prompt.txt
```

You can also get the skill content from the CLI: `coast skills-prompt`.

After setup, **quit and reopen your editor** for the new skill and project
instructions to take effect.

---

[Shep](https://shep-ai.github.io/cli/) creates worktrees at `~/.shep/repos/{hash}/wt/{branch-slug}`. The hash is the first 16 hex characters of the SHA-256 of the repository's absolute path, so it is deterministic per-repo but opaque. All worktrees for a given repo share the same hash and are differentiated by the `wt/{branch-slug}` subdirectory.

From the Shep CLI, `shep feat show <feature-id>` prints the worktree path, or
`ls ~/.shep/repos` lists the per-repo hash directories.

Because the hash varies per repo, Coasts uses a **glob pattern** to discover
shep worktrees without requiring the user to hard-code the hash.

## Setup

Add `~/.shep/repos/*/wt` to `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

The `*` matches the per-repo hash directory. At runtime Coasts expands the glob,
finds the matching directory (e.g. `~/.shep/repos/a21f0cda9ab9d456/wt`), and
bind-mounts it into the container. See
[Worktree Directories](../coastfiles/WORKTREE_DIR.md) for full details on glob
patterns.

After changing `worktree_dir`, existing instances must be **recreated** for the bind mount to take effect:

```bash
coast rm my-instance
coast build
coast run my-instance
```

The worktree listing updates immediately (Coasts reads the new Coastfile), but
assigning to a Shep worktree requires the bind mount inside the container.

## Where Coasts guidance goes

Shep wraps Claude Code under the hood, so follow the Claude Code conventions:

- put the short Coast Runtime rules in `CLAUDE.md`
- put the reusable `/coasts` workflow in `.claude/skills/coasts/SKILL.md` or
  the shared `.agents/skills/coasts/SKILL.md`
- if this repo also uses other harnesses, see
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) and
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)

## What Coasts does

- **Run** -- `coast run <name>` creates a new Coast instance from the latest build. Use `coast run <name> -w <worktree>` to create and assign a Shep worktree in one step. See [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- At container creation, Coasts resolves the glob
  `~/.shep/repos/*/wt` and mounts each matching directory into the container at
  `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` is repo-scoped, so only
  worktrees belonging to the current project appear.
- **Naming** -- Shep worktrees use named branches, so they appear by branch
  name in the Coasts UI and CLI (e.g., `feat-green-background`).
- **Assign** -- `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** -- Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** -- The git watcher scans external directories
  recursively, filtering by `.git` gitdir pointers. If Shep deletes a
  worktree, Coasts auto-unassigns the instance.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `~/.shep/repos/*/wt` -- Shep (external, bind-mounted via glob expansion)

## Shep path structure

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

Key points:
- Same repo = same hash every time (deterministic, not random)
- Different repos = different hashes
- Path separators are normalized to `/` before hashing
- The hash can be found via `shep feat show <feature-id>` or `ls ~/.shep/repos`

## Troubleshooting

- **Worktree not found** — If Coasts expects a worktree to exist but cannot
  find it, verify that the Coastfile's `worktree_dir` includes
  `~/.shep/repos/*/wt`. The glob pattern must match Shep's directory structure.
  See [Worktree Directories](../coastfiles/WORKTREE_DIR.md) for syntax and
  path types.

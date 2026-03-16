# Harnesses

Most harnesses create git worktrees to run tasks in parallel. These worktrees may live inside your project or outside it entirely. Coast's [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) array tells it where to look -- including external paths like `~/.codex/worktrees` that require additional bind mounts.

Each page below covers the Coastfile configuration and any caveats specific to that harness.

| Harness | Worktree location | Page |
|---------|-------------------|------|
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |

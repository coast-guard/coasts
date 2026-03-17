# Harnesses

Большинство harnesses создают git worktree, чтобы запускать задачи параллельно. Эти worktree могут находиться внутри вашего проекта или полностью вне его. Массив [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) в Coast указывает, где их искать, включая внешние пути, такие как `~/.codex/worktrees`, для которых требуются дополнительные bind mounts.

На каждой странице ниже описаны конфигурация Coastfile и любые особенности, относящиеся именно к этому harness.

| Harness | Worktree location | Page |
|---------|-------------------|------|
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |
| Conductor | `~/conductor/workspaces/<project>` | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | [T3 Code](T3_CODE.md) |

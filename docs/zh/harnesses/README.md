# Harnesses

大多数 harness 会创建 git worktree 来并行运行任务。这些 worktree 可能位于你的项目内部，也可能完全位于项目外部。Coast 的 [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 数组会告诉它去哪里查找——包括像 `~/.codex/worktrees` 这样的外部路径，这些路径需要额外的 bind mount。

下面的每一页都介绍了该 harness 特有的 Coastfile 配置以及任何注意事项。

| Harness | Worktree location | Page |
|---------|-------------------|------|
| Conductor | `~/conductor/workspaces/<project>` | [Conductor](CONDUCTOR.md) |
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |
| T3 Code | `~/.t3/worktrees/<project>` | [T3 Code](T3_CODE.md) |

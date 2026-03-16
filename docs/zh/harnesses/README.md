# Harnesses

大多数 harness 会创建 git worktree 以并行运行任务。这些 worktree 可能位于你的项目内部，也可能完全位于项目之外。Coast 的 [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 数组会告诉它去哪里查找——包括像 `~/.codex/worktrees` 这样的外部路径，这些路径需要额外的 bind mount。

下面的每个页面都介绍了 Coastfile 配置以及该 harness 特有的任何注意事项。

| Harness | Worktree location | Page |
|---------|-------------------|------|
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |

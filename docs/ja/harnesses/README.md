# Harnesses

ほとんどの harness は、タスクを並列で実行するために git worktree を作成します。これらの worktree は、あなたのプロジェクト内に存在する場合もあれば、完全に外部に存在する場合もあります。Coast の [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 配列は、追加の bind mount を必要とする `~/.codex/worktrees` のような外部パスを含め、どこを探すかを指定します。

以下の各ページでは、その harness に固有の Coastfile 設定と注意点について説明します。

| Harness | Worktree location | Page |
|---------|-------------------|------|
| Conductor | `~/conductor/workspaces/<project>` | [Conductor](CONDUCTOR.md) |
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |
| T3 Code | `~/.t3/worktrees/<project>` | [T3 Code](T3_CODE.md) |

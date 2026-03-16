# ハーネス

ほとんどのハーネスは、タスクを並列に実行するために git worktree を作成します。これらの worktree は、プロジェクト内に存在する場合もあれば、完全に外部に存在する場合もあります。Coast の [`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 配列は、追加の bind mount が必要な `~/.codex/worktrees` のような外部パスを含め、どこを探すかを指定します。

以下の各ページでは、そのハーネスに固有の Coastfile 設定と注意点を説明します。

| Harness | Worktree location | Page |
|---------|-------------------|------|
| OpenAI Codex | `~/.codex/worktrees` | [Codex](CODEX.md) |

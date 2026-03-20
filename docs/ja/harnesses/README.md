# Harnesses

各 harness は異なる場所に git worktree を作成します。Coasts では、
[`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 配列によって、どこを探すかを指定します --
これには、追加の bind mount を必要とする `~/.codex/worktrees` のような
外部パスも含まれます。

各 harness には、プロジェクトレベルの instructions、skills、commands について、それぞれ独自の慣習もあります。以下のマトリクスは各 harness が何をサポートしているかを示しており、Coasts のガイダンスをどこに置けばよいかが分かります。各ページでは、Coastfile の設定、推奨されるファイルレイアウト、その harness に固有の注意点を扱います。

1 つのリポジトリを複数の harness から使用する場合は、[Multiple Harnesses](MULTIPLE_HARNESSES.md) を参照してください。

| Harness | Worktree location | Project instructions | Skills | Commands | Page |
|---------|-------------------|----------------------|--------|----------|------|
| OpenAI Codex | `~/.codex/worktrees` | `AGENTS.md` | `.agents/skills/` | Skills surface as `/` commands | [Codex](CODEX.md) |
| Claude Code | `.claude/worktrees` | `CLAUDE.md` | `.claude/skills/` | `.claude/commands/` | [Claude Code](CLAUDE_CODE.md) |
| Cursor | `~/.cursor/worktrees/<project>` | `AGENTS.md` or `.cursor/rules/` | `.cursor/skills/` or `.agents/skills/` | `.cursor/commands/` | [Cursor](CURSOR.md) |
| Conductor | `~/conductor/workspaces/<project>` | `CLAUDE.md` | -- | -- | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | `AGENTS.md` | `.agents/skills/` | -- | [T3 Code](T3_CODE.md) |

## Skills vs Commands

Skills と commands はどちらも、再利用可能な `/coasts` ワークフローを定義できます。使っている harness が何をサポートしているかに応じて、どちらか一方、または両方を使えます。

harness が commands をサポートしていて、明示的な `/coasts`
entrypoint が欲しい場合、簡単な方法の 1 つは、skill を再利用する command を追加することです。
Commands は名前で明示的に呼び出されるため、
`/coasts` ワークフローがいつ実行されるかを正確に把握できます。Skills は agent によって
コンテキストに応じて自動的に読み込まれることもあり、
これは便利ですが、その instructions がいつ取り込まれるかについての制御は少なくなります。

両方を使うこともできます。その場合は、ワークフローの別コピーを
個別に管理するのではなく、command から skill を再利用するようにしてください。

harness が skills のみをサポートしている場合（T3 Code）は、skill を使ってください。どちらもサポートしていない場合（Conductor）は、
`/coasts` ワークフローをプロジェクトの instructions ファイルに直接記述してください。

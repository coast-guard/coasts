# Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) は
`.claude/worktrees/` にあるプロジェクト内で worktree を作成します。そのディレクトリは
リポジトリ内に存在するため、Coasts は外部の bind mount なしで Claude Code の worktree を
検出して割り当てることができます。

Claude Code はまた、このドキュメントにおいて Coasts 向けの3つのレイヤーが最も明確に
分かれているハーネスでもあります:

- Coasts を操作するための短く常時有効なルールには `CLAUDE.md`
- 再利用可能な `/coasts` ワークフローには `.claude/skills/coasts/SKILL.md`
- 追加のエントリポイントとしてコマンドファイルを使いたい場合のみ `.claude/commands/coasts.md`

## Setup

`worktree_dir` に `.claude/worktrees` を追加します:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

`.claude/worktrees` はプロジェクト相対であるため、外部 bind mount は
必要ありません。

## Where Coasts guidance goes

### `CLAUDE.md`

すべてのタスクに適用されるべき Coasts のルールはここに置きます。短く、
運用的な内容にしてください:

- セッション内で最初のランタイムコマンドを実行する前に `coast lookup` を実行する
- テスト、ビルド、サービスコマンドには `coast exec` を使う
- ランタイムのフィードバックには `coast ps` と `coast logs` を使う
- 一致するものが存在しない場合、Coast の作成または再割り当ての前に確認する

### `.claude/skills/coasts/SKILL.md`

再利用可能な `/coasts` ワークフローはここに置きます。これは次のようなフローに
適した場所です:

1. `coast lookup` を実行し、一致する Coast を再利用する
2. 一致するものがない場合は `coast ls` にフォールバックする
3. `coast run`, `coast assign`, `coast unassign`, `coast checkout`, および
   `coast ui` を提示する
4. ラップするのではなく、契約として Coast CLI を直接使う

このリポジトリが Codex、T3 Code、または Cursor も使う場合は、
[Multiple Harnesses](MULTIPLE_HARNESSES.md) を参照し、正規の skill を
`.agents/skills/coasts/` に置いてから、それを Claude Code に公開してください。

### `.claude/commands/coasts.md`

Claude Code はプロジェクトコマンドファイルもサポートしています。Coasts のドキュメントでは、
これは任意のものとして扱ってください:

- 明確にコマンドファイルが必要な場合にのみ使う
- 単純な選択肢の1つは、そのコマンドに同じ skill を再利用させること
- コマンドに独自の別個の指示を持たせる場合、保守すべきワークフローの
  2つ目のコピーを抱えることになります

## Example layout

### Claude Code only

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

このリポジトリが Codex、T3 Code、または Cursor も使う場合は、ここで重複させるのではなく
[Multiple Harnesses](MULTIPLE_HARNESSES.md) の共有パターンを使ってください。
プロバイダ固有のガイダンスを重複させると、別のハーネスを追加するたびに同期を保つのが
難しくなるためです。

## What Coasts does

- **Run** — `coast run <name>` は最新のビルドから新しい Coast インスタンスを作成します。`coast run <name> -w <worktree>` を使うと、Claude Code の worktree を作成して1ステップで割り当てられます。[Run](../concepts_and_terminology/RUN.md) を参照してください。
- **Discovery** — Coasts は他のローカル worktree ディレクトリと同様に `.claude/worktrees` を読み取ります。
- **Naming** — Claude Code の worktree は、Coasts UI および CLI において、他のリポジトリ内 worktree と同じローカル worktree 命名動作に従います。
- **Assign** — `coast assign` は、外部 bind-mount の間接層なしで `/workspace` を Claude Code の worktree に切り替えられます。
- **Gitignored sync** — worktree はリポジトリツリー内に存在するため、通常どおり動作します。
- **Orphan detection** — Claude Code が worktree を削除した場合、Coasts は不足している gitdir を検出し、必要に応じてその割り当てを解除できます。

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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

- `.claude/worktrees/` — Claude Code の worktree
- `~/.codex/worktrees/` — このリポジトリで Codex も使う場合の Codex worktree

## Limitations

- `/coasts` ワークフローを `CLAUDE.md`、`.claude/skills`、および `.claude/commands` にまたがって重複させると、それらのコピーは乖離していきます。`CLAUDE.md` は短く保ち、再利用可能なワークフローは1つの skill にまとめてください。
- 1つのリポジトリを複数のハーネスで適切に動作させたい場合は、[Multiple Harnesses](MULTIPLE_HARNESSES.md) の共有パターンを優先してください。

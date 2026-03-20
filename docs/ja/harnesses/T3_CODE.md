# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) は `~/.t3/worktrees/<project-name>/` に git worktree を作成し、名前付きブランチにチェックアウトします。

T3 Code では、常時有効な Coast Runtime ルールを `AGENTS.md` に置き、再利用可能な `/coasts` ワークフローを `.agents/skills/coasts/SKILL.md` に置きます。

これらの worktree はプロジェクトルートの外に存在するため、Coasts がそれらを検出してマウントするには明示的な設定が必要です。

## Setup

`~/.t3/worktrees/<project-name>` を `worktree_dir` に追加します。T3 Code は worktree をプロジェクトごとのサブディレクトリ配下にネストするため、パスにはプロジェクト名を含める必要があります。以下の例では、`my-app` はあなたのリポジトリに対する `~/.t3/worktrees/` 配下の実際のフォルダ名と一致している必要があります。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coasts は実行時に `~` を展開し、`~/` または `/` で始まる任意のパスを外部として扱います。詳細は [Worktree Directories](../coastfiles/WORKTREE_DIR.md) を参照してください。

`worktree_dir` を変更した後、バインドマウントを有効にするには既存のインスタンスを**再作成**する必要があります。

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree の一覧は即座に更新されます（Coasts は新しい Coastfile を読み取ります）が、T3 Code の worktree への割り当てにはコンテナ内のバインドマウントが必要です。

## Where Coasts guidance goes

T3 Code では次のレイアウトを使用してください。

- 短い Coast Runtime ルールは `AGENTS.md` に置く
- 再利用可能な `/coasts` ワークフローは `.agents/skills/coasts/SKILL.md` に置く
- Coasts 用に T3 固有の別個のプロジェクトコマンドまたはスラッシュコマンド層を追加しない
- このリポジトリが複数のハーネスを使用している場合は、
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) と
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) を参照してください。

## What Coasts does

- **Run** — `coast run <name>` は最新のビルドから新しい Coast インスタンスを作成します。`coast run <name> -w <worktree>` を使うと、T3 Code の worktree を作成して 1 ステップで割り当てられます。詳細は [Run](../concepts_and_terminology/RUN.md) を参照してください。
- **Bind mount** — コンテナ作成時に、Coasts は
  `~/.t3/worktrees/<project-name>` をコンテナ内の
  `/host-external-wt/{index}` にマウントします。
- **Discovery** — `git worktree list --porcelain` はリポジトリスコープであるため、現在のプロジェクトに属する worktree のみが表示されます。
- **Naming** — T3 Code の worktree は名前付きブランチを使用するため、Coasts の UI と CLI ではブランチ名で表示されます。
- **Assign** — `coast assign` は `/workspace` を外部バインドマウントパスから再マウントします。
- **Gitignored sync** — ホストファイルシステム上で絶対パスを使って実行され、バインドマウントなしで動作します。
- **Orphan detection** — git watcher は外部ディレクトリを再帰的にスキャンし、`.git` の gitdir ポインタでフィルタリングします。T3 Code がワークスペースを削除した場合、Coasts はインスタンスの割り当てを自動的に解除します。

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
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

- `.claude/worktrees/` — Claude Code（ローカル、特別な処理なし）
- `~/.codex/worktrees/` — Codex（外部、バインドマウントされる）
- `~/.t3/worktrees/my-app/` — T3 Code（外部、バインドマウントされる。`my-app` はあなたのリポジトリフォルダ名に置き換えてください）

## Limitations

- Coasts 内のランタイム設定に T3 Code 固有の環境変数へ依存することは避けてください。Coasts はポート、ワークスペースパス、サービスディスカバリを独立して管理します — 代わりに Coastfile の `[ports]` と `coast exec` を使用してください。

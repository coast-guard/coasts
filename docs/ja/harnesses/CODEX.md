# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) は `$CODEX_HOME/worktrees`（通常は `~/.codex/worktrees`）に worktree を作成します。各 worktree は `~/.codex/worktrees/a0db/project-name` のような不透明なハッシュのディレクトリ配下に存在し、detached HEAD で開始され、Codex の保持ポリシーに基づいて自動的にクリーンアップされます。

[Codex docs](https://developers.openai.com/codex/app/worktrees/) より:

> worktree が作成される場所を制御できますか？
> 現時点ではできません。Codex は一貫して管理できるように、`$CODEX_HOME/worktrees` 配下に worktree を作成します。

これらの worktree はプロジェクトルートの外側に存在するため、Coast がそれらを検出してマウントするには明示的な設定が必要です。

## Setup

`worktree_dir` に `~/.codex/worktrees` を追加します:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coast は実行時に `~` を展開し、`~/` または `/` で始まるパスを外部として扱います。詳細は [Worktree Directories](../coastfiles/WORKTREE_DIR.md) を参照してください。

`worktree_dir` を変更した後は、バインドマウントを有効にするために既存のインスタンスを**再作成**する必要があります:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree の一覧はすぐに更新されます（Coast は新しい Coastfile を読み込みます）が、Codex worktree への割り当てにはコンテナ内のバインドマウントが必要です。

## What Coast does

- **Bind mount** -- コンテナ作成時に、Coast は `~/.codex/worktrees` をコンテナ内の `/host-external-wt/{index}` にマウントします。
- **Discovery** -- `git worktree list --porcelain` はリポジトリスコープであるため、そのディレクトリに多くのプロジェクトの worktree が含まれていても、現在のプロジェクトに属する Codex worktree のみが表示されます。
- **Naming** -- Detached HEAD の worktree は外部ディレクトリ内での相対パス（`a0db/my-app`, `eca7/my-app`）として表示されます。ブランチベースの worktree はブランチ名として表示されます。
- **Assign** -- `coast assign` は外部バインドマウントパスから `/workspace` を再マウントします。
- **Gitignored sync** -- ホストファイルシステム上で絶対パスを使って実行されるため、バインドマウントなしでも動作します。
- **Orphan detection** -- git watcher は外部ディレクトリを再帰的にスキャンし、`.git` の gitdir ポインタでフィルタします。Codex が worktree を削除した場合、Coast はインスタンスの割り当てを自動的に解除します。

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

- `.worktrees/` -- Coast 管理の worktree
- `.claude/worktrees/` -- Claude Code（ローカル、特別な処理なし）
- `~/.codex/worktrees/` -- Codex（外部、バインドマウントされる）

## Limitations

- Coast は Codex worktree を検出してマウントしますが、それらを作成または削除はしません。
- Codex はいつでも worktree をクリーンアップする可能性があります。Coast の orphan detection はこれを適切に処理します。
- `coast assign` によって作成される新しい worktree は、常にローカルの `default_worktree_dir` に作成され、外部ディレクトリには作成されません。

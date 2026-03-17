# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) は Ping によるオープンソースのコーディングエージェントハーネスです。各ワークスペースは `~/.t3/worktrees/<project-name>/` に保存された git worktree で、名前付きブランチにチェックアウトされます。

これらの worktree はプロジェクトルートの外に存在するため、Coast がそれらを検出してマウントするには明示的な設定が必要です。

## Setup

`~/.t3/worktrees/<project-name>` を `worktree_dir` に追加します。T3 Code は worktree をプロジェクトごとのサブディレクトリ配下にネストするため、パスにはプロジェクト名を含める必要があります。以下の例では、`my-app` はあなたのリポジトリに対する `~/.t3/worktrees/` 配下の実際のフォルダ名と一致している必要があります。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coast は実行時に `~` を展開し、`~/` または `/` で始まる任意のパスを外部として扱います。詳細は [Worktree Directories](../coastfiles/WORKTREE_DIR.md) を参照してください。

`worktree_dir` を変更した後、バインドマウントを有効にするには既存のインスタンスを**再作成**する必要があります。

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree の一覧は即座に更新されます（Coast は新しい Coastfile を読み取ります）が、T3 Code の worktree への割り当てにはコンテナ内のバインドマウントが必要です。

## What Coast does

- **Bind mount** — コンテナ作成時に、Coast は `~/.t3/worktrees/<project-name>` をコンテナ内の `/host-external-wt/{index}` にマウントします。
- **Discovery** — `git worktree list --porcelain` はリポジトリスコープであるため、現在のプロジェクトに属する worktree のみが表示されます。
- **Naming** — T3 Code の worktree は名前付きブランチを使用するため、Coast UI と CLI ではブランチ名で表示されます。
- **Assign** — `coast assign` は `/workspace` を外部バインドマウントパスから再マウントします。
- **Gitignored sync** — ホストファイルシステム上で絶対パスを使って実行され、バインドマウントなしで動作します。
- **Orphan detection** — git watcher は外部ディレクトリを再帰的にスキャンし、`.git` の gitdir ポインタでフィルタリングします。T3 Code がワークスペースを削除した場合、Coast はインスタンスの割り当てを自動的に解除します。

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

- `.worktrees/` — Coast 管理の worktree
- `.claude/worktrees/` — Claude Code（ローカル、特別な処理なし）
- `~/.codex/worktrees/` — Codex（外部、バインドマウントされる）
- `~/.t3/worktrees/my-app/` — T3 Code（外部、バインドマウントされる。`my-app` はあなたのリポジトリフォルダ名に置き換えてください）

## Limitations

- Coast は T3 Code の worktree を検出してマウントしますが、それらを作成または削除はしません。
- `coast assign` によって作成される新しい worktree は常にローカルの `default_worktree_dir` に配置され、外部ディレクトリには作成されません。
- Coasts 内のランタイム設定に T3 Code 固有の環境変数へ依存することは避けてください。Coast はポート、ワークスペースパス、サービスディスカバリを独立して管理します — 代わりに Coastfile の `[ports]` と `coast exec` を使用してください。

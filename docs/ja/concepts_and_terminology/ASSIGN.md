# 割り当てと割り当て解除

割り当てと割り当て解除は、Coast インスタンスがどの worktree を指すかを制御します。マウントレベルでの worktree 切り替えの仕組みについては [Filesystem](FILESYSTEM.md) を参照してください。

## 割り当て

`coast assign` は、Coast インスタンスを特定の worktree に切り替えます。Coast は worktree がまだ存在しない場合は作成し、Coast 内のコードを更新し、設定された割り当て戦略に従ってサービスを再起動します。

```bash
coast assign dev-1 --worktree feature/oauth
```

```text
Before:
┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘

coast assign dev-1 --worktree feature/oauth

After:
┌─── dev-1 ──────────────────┐
│  branch: feature/oauth     │
│  worktree: feature/oauth   │
│                            │
│  postgres → skipped (none) │
│  web      → hot swapped    │
│  api      → restarted      │
│  worker   → rebuilt        │
└────────────────────────────┘
```

割り当て後、`dev-1` は `feature/oauth` ブランチを実行し、そのすべてのサービスが起動した状態になります。

## 割り当て解除

`coast unassign` は、Coast インスタンスをプロジェクトルート（main/master ブランチ）に戻します。worktree の関連付けが削除され、Coast はプライマリリポジトリから実行する状態に戻ります。

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## 割り当て戦略

Coast が新しい worktree に割り当てられると、各サービスはコード変更をどのように扱うかを知る必要があります。これは [Coastfile](COASTFILE_TYPES.md) の `[assign]` でサービスごとに設定します。

```toml
[assign]
default = "restart"

[assign.services]
postgres = "none"
redis = "none"
web = "hot"
worker = "rebuild"
```

```text
coast assign dev-1 --worktree feature/billing

  postgres (strategy: none)    →  skipped, unchanged between branches
  redis (strategy: none)       →  skipped, unchanged between branches
  web (strategy: hot)          →  filesystem swapped, file watcher picks it up
  api (strategy: restart)      →  container restarted
  worker (strategy: rebuild)   →  image rebuilt, container restarted
```

利用可能な戦略は次のとおりです:

- **none** — 何もしません。Postgres や Redis のように、ブランチ間で変化しないサービスに使用します。
- **hot** — ファイルシステムのみを入れ替えます。サービスは起動したままで、マウント伝播とファイルウォッチャー（例: ホットリロード付きの開発サーバー）により変更を取り込みます。
- **restart** — サービスコンテナを再起動します。プロセスの再起動だけが必要なインタプリタ系サービスに使用します。これがデフォルトです。
- **rebuild** — サービスイメージを再ビルドして再起動します。ブランチ変更が `Dockerfile` やビルド時依存関係に影響する場合に使用します。

また、特定のファイルが変更されたときだけサービスを再ビルドするように、再ビルドトリガーを指定することもできます:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

ブランチ間でトリガーファイルのいずれも変更されていない場合、戦略が `rebuild` に設定されていても、サービスは再ビルドをスキップします。

## 削除された Worktree

割り当てられた worktree が削除された場合、`coastd` デーモンはそのインスタンスを自動的に割り当て解除し、メインの Git リポジトリルートに戻します。

---

> **Tip: 大規模コードベースで割り当ての待ち時間を短縮する**
>
> 内部的に、Coast は worktree がマウントまたはアンマウントされるたびに `git ls-files` を実行します。大規模なコードベースや多数のファイルを含むリポジトリでは、これが割り当て／割り当て解除操作に目立つ遅延を追加することがあります。
>
> コードベースの一部が割り当て間で再ビルドされる必要がない場合は、Coastfile の `exclude_paths` を使ってそれらをスキップするよう Coast に指示できます:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> `exclude_paths` に列挙されたパスはファイル差分の間に無視されるため、割り当て時間を大幅に短縮できます。

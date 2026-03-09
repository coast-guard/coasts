# 割り当てと割り当て解除

割り当て（assign）と割り当て解除（unassign）は、Coast インスタンスがどの worktree を指すかを制御します。マウントレベルでの worktree 切り替えの仕組みについては [Filesystem](FILESYSTEM.md) を参照してください。

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

割り当て後、`dev-1` は `feature/oauth` ブランチで稼働し、すべてのサービスが起動した状態になります。

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

Coast が新しい worktree に割り当てられると、各サービスはコード変更をどのように扱うかを把握する必要があります。これは [Coastfile](COASTFILE_TYPES.md) の `[assign]` 配下でサービスごとに設定します。

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

利用可能な戦略は次のとおりです。

- **none** — 何もしません。Postgres や Redis のようにブランチ間で変化しないサービスに使用します。
- **hot** — ファイルシステムのみを入れ替えます。サービスは稼働したままで、マウントの伝播とファイルウォッチャー（例: ホットリロード付きの開発サーバー）により変更を取り込みます。
- **restart** — サービスコンテナを再起動します。プロセスの再起動だけが必要なインタプリタ系サービスに使用します。これがデフォルトです。
- **rebuild** — サービスイメージを再ビルドして再起動します。ブランチ変更が `Dockerfile` やビルド時依存関係に影響する場合に使用します。

また、特定のファイルが変更されたときだけサービスが再ビルドされるように、rebuild トリガーを指定することもできます。

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

ブランチ間でトリガーファイルがどれも変更されていない場合、戦略が `rebuild` に設定されていても、そのサービスは再ビルドをスキップします。

## 削除された Worktree

割り当てられている worktree が削除された場合、`coastd` デーモンはそのインスタンスを自動的にメインの Git リポジトリルートへ割り当て解除します。

---

> **ヒント: 大規模コードベースで assign の待ち時間を削減する**
>
> 内部的には、新しい worktree への最初の assign で、選択された gitignored ファイルがその worktree にブートストラップされ、`[assign.rebuild_triggers]` を持つサービスは再ビルドが必要かどうか判断するために `git diff --name-only` を実行する場合があります。大規模コードベースでは、このブートストラップ手順と不要な再ビルドが assign 時間の大半を占めがちです。
>
> Coastfile の `exclude_paths` を使って gitignored のブートストラップ対象範囲を縮小し、ファイルウォッチャーを持つサービスには `"hot"` を使用し、`[assign.rebuild_triggers]` は真のビルド時入力に絞ってください。既存の worktree に対して ignored-file のブートストラップを手動で更新する必要がある場合は、`coast assign --force-sync` を実行します。完全なガイドは [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) を参照してください。

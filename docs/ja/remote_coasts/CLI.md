# CLI と設定

このページでは、`coast remote` コマンドグループ、`Coastfile.remote` 設定フォーマット、およびリモートマシンのディスク管理について説明します。

## リモート管理コマンド

### `coast remote add`

デーモンにリモートマシンを登録します:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

例:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

接続の詳細はデーモンの `state.db` に保存されます。これらが Coastfile に保存されることはありません。

### `coast remote ls`

登録済みのすべてのリモートを一覧表示します:

```bash
coast remote ls
```

### `coast remote rm`

登録済みのリモートを削除します:

```bash
coast remote rm <name>
```

リモート上でインスタンスがまだ実行中の場合は、まず `coast rm` でそれらを削除してください。

### `coast remote test`

SSH 接続性と coast-service の可用性を検証します:

```bash
coast remote test <name>
```

これは SSH アクセスを確認し、SSH トンネル経由でポート 31420 上の coast-service に到達可能であることを確認し、リモートのアーキテクチャと coast-service のバージョンを報告します。

### `coast remote prune`

リモートマシン上の孤立したリソースをクリーンアップします:

```bash
coast remote prune <name>              # orphaned resources を削除
coast remote prune <name> --dry-run    # 削除される内容をプレビュー
```

prune は、Docker ボリュームとワークスペースディレクトリを coast-service のインスタンスデータベースと照合することで、孤立したリソースを特定します。アクティブなインスタンスに属するリソースが削除されることはありません。

## Coastfile 設定

リモート coast は、ベース設定を拡張する別個の Coastfile を使用します。ファイル名によってタイプが決まります:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### 最小例

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### `[remote]` セクション

`[remote]` セクションは同期設定を宣言します。接続の詳細（host、user、SSH key）は `coast remote add` から取得され、実行時に解決されます。

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | 同期戦略: 一度だけの一括転送のみを行う `"rsync"`、または rsync + 継続的なリアルタイム同期を行う `"mutagen"` |

### 検証制約

1. Coastfile のタイプが `remote` で始まる場合、`[remote]` セクションは必須です。
2. リモートではない Coastfile には `[remote]` セクションを含めることはできません。
3. インラインの host 設定はサポートされていません。接続の詳細は登録済みリモートから取得する必要があります。
4. `strategy = "shared"` を持つ共有ボリュームは、リモートホスト上に Docker ボリュームを作成し、そのリモート上のすべての coast 間で共有されます。このボリュームは異なるリモートマシン間では分散されません。

### 継承

リモート Coastfile は、他の型付き Coastfile と同じ [継承システム](../coastfiles/INHERITANCE.md) を使用します。`extends = "Coastfile"` ディレクティブは、ベース設定とリモートのオーバーライドをマージします。ポート、サービス、ボリュームをオーバーライドし、他の型付きバリアントと同様に戦略を割り当てることができます。

## ディスク管理

### インスタンスごとのリソース使用量

各リモート coast インスタンスは、おおよそ次を消費します:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

推奨される最小ディスク容量: 一般的なプロジェクトで 2～3 個の同時実行インスタンスを想定して **50 GB**。

### リソース命名規則

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### `coast rm` 時のクリーンアップ

`coast rm` がリモートインスタンスを削除するとき、以下をクリーンアップします:

1. リモート DinD コンテナ（coast-service 経由）
2. DinD Docker ボリューム（`coast-dind--{project}--{name}`）
3. ワークスペースディレクトリ（`/data/workspaces/{project}/{name}`）
4. ローカルのシャドウインスタンスレコード、ポート割り当て、およびシェルコンテナ

### prune を実行すべきタイミング

インスタンスを削除した後でもリモート上の `df -h` が高いディスク使用量を示す場合、失敗または中断された操作により孤立したリソースが残っている可能性があります。領域を回収するには `coast remote prune` を実行してください:

```bash
# 削除される内容を確認
coast remote prune my-vm --dry-run

# 実際に削除
coast remote prune my-vm
```

prune はアクティブなインスタンスに属するリソースを削除しません。

# リモート

リモート coast は、ラップトップの代わりにリモートマシン上でサービスを実行します。CLI と UI の体験はローカルの coast と同一であり、`coast run`、`coast assign`、`coast exec`、`coast ps`、`coast checkout` はすべて同じように動作します。デーモンはインスタンスがリモートであることを検出し、SSH トンネルを介してリモートホスト上の `coast-service` に操作をルーティングします。

## Local vs Remote

| | ローカル Coast | リモート Coast |
|---|---|---|
| DinD コンテナ | あなたのマシン上で実行 | リモートマシン上で実行 |
| Compose サービス | ローカル DinD 内 | リモート DinD 内 |
| ファイル編集 | 直接バインドマウント | シェル coast（ローカル）+ rsync/mutagen 同期 |
| ポートアクセス | `socat` フォワーダー | SSH `-L` トンネル + `socat` フォワーダー |
| 共有サービス | ブリッジネットワーク | SSH `-R` リバーストンネル |
| ビルドアーキテクチャ | あなたのマシンのアーキテクチャ | リモートマシンのアーキテクチャ |

## How It Works

すべてのリモート coast は 2 つのコンテナを作成します。

1. ローカルマシン上の **シェル coast**。これは通常の coast と同じバインドマウント（`/host-project`、`/workspace`）を持つ軽量な Docker コンテナ（`sleep infinity`）です。これは、ホストエージェントがリモートに同期されるファイルを編集できるようにするために存在します。

2. リモートマシン上の **リモート coast**。これは `coast-service` によって管理されます。これは動的ポートを使用して、compose サービスを含む実際の DinD コンテナを実行します。

デーモンは SSH トンネルでそれらを橋渡しします。

- **フォワードトンネル**（`ssh -L`）: 各ローカル動的ポートを対応するリモート動的ポートにマッピングし、`localhost:{dynamic}` がリモートサービスに到達するようにします。
- **リバーストンネル**（`ssh -R`）: ローカルの[共有サービス](SHARED_SERVICES.md)（Postgres、Redis）をリモート DinD コンテナに公開します。

## Registering Remotes

リモートはデーモンに登録され、`state.db` に保存されます。

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/coast_key
coast remote test my-vm
coast remote ls
coast remote rm my-vm
```

接続の詳細（ホスト、ユーザー、ポート、SSH キー）は Coastfile ではなく、デーモンのデータベースに保存されます。Coastfile は `[remote]` セクションを通じて同期設定のみを宣言します。

## Remote Builds

ビルドはリモートマシン上で行われるため、イメージはリモートのネイティブアーキテクチャを使用します。ARM Mac はクロスコンパイルなしで x86_64 リモート上に x86_64 イメージをビルドできます。

ビルド後、成果物は再利用のためにローカルマシンへ転送されます。別のリモートが同じアーキテクチャを持っている場合、事前ビルド済みの成果物を再ビルドせずに直接デプロイできます。ビルド成果物の構造についての詳細は [Builds](BUILDS.md) を参照してください。

## File Sync

リモート coast は、初回の一括転送に rsync を使用し、継続的なリアルタイム同期に mutagen を使用します。両方のツールは、ホストマシン上ではなく coast コンテナ（シェル coast と coast-service イメージ）内で実行されます。同期設定の詳細については、[Remote Coasts](../remote_coasts/README.md) ガイドを参照してください。

## Disk Management

リモートマシンには Docker ボリューム、ワークスペースディレクトリ、イメージ tarball が蓄積されます。`coast rm` がリモートインスタンスを削除すると、関連するすべてのリソースがクリーンアップされます。失敗した操作による孤立したリソースについては、`coast remote prune` を使用してください。

## Setup

ホスト要件、coast-service のデプロイ、Coastfile の設定を含む完全なセットアップ手順については、[Remote Coasts](../remote_coasts/README.md) ガイドを参照してください。

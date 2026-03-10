# 共有サービス

共有サービスは、Coast の内部ではなく、ホストの Docker デーモン上で実行されるデータベースおよびインフラ用コンテナ（Postgres、Redis、MongoDB など）です。Coast インスタンスはブリッジネットワーク経由でそれらに接続するため、すべての Coast が同じホストボリューム上の同じサービスと通信します。

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*ホスト管理の Postgres、Redis、MongoDB を表示している Coastguard の共有サービスタブ。*

## 仕組み

Coastfile で共有サービスを宣言すると、Coast はそれをホストデーモン上で起動し、各 Coast コンテナ内で実行される compose スタックからは取り除きます。その後、Coast はサービス名宛てのトラフィックを共有コンテナへ戻すように設定され、同時に Coast 内ではそのサービスのコンテナ側ポートが維持されます。

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

共有サービスは既存のホストボリュームを再利用するため、ローカルで `docker-compose up` を実行してすでに存在しているデータは、すぐに Coasts から利用できます。

この違いは、マップされたポートを使用する場合に重要になります。

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- ホスト上では、共有サービスは `localhost:5433` で公開されます。
- 各 Coast の内部では、アプリコンテナは引き続き `postgis:5432` に接続します。
- `5432` のような単独の整数は、同一マッピング `"5432:5432"` の省略記法です。

## 共有サービスを使うべきタイミング

- プロジェクトにローカルデータベースへ接続する MCP 統合がある場合、共有サービスを使うことで動的なポート検出なしでもそれらを引き続き利用できます。共有サービスを、ツールがすでに使用しているのと同じホストポートで公開する場合（たとえば `ports = [5432]`）、それらのツールは変更なしで動作し続けます。別のホストポートで公開する場合（たとえば `"5433:5432"`）、ホスト側ツールはそのホストポートを使用し、Coasts は引き続きコンテナポートを使用します。
- 各 Coast インスタンスが独自のデータベースコンテナを実行する必要がないため、より軽量な Coast インスタンスにしたい場合。
- Coast インスタンス間でデータ分離が不要な場合（すべてのインスタンスが同じデータを参照します）。
- ホスト上でコーディングエージェントを実行していて（[Filesystem](FILESYSTEM.md) を参照）、[`coast exec`](EXEC_AND_DOCKER.md) を経由せずにそれらからデータベースの状態へアクセスしたい場合。共有サービスを使えば、エージェントの既存のデータベースツールや MCP は変更なしで動作します。

分離が必要な場合の代替手段については、[Volume Topology](VOLUMES.md) ページを参照してください。

## ボリュームの曖昧性に関する警告

Docker のボリューム名は、常にグローバルに一意であるとは限りません。複数の異なるプロジェクトから `docker-compose up` を実行している場合、Coast が共有サービスに接続するホストボリュームが、想定しているものではない可能性があります。

共有サービス付きで Coasts を起動する前に、最後に実行した `docker-compose up` が、Coasts で使用するつもりのプロジェクトからのものであることを確認してください。これにより、ホストボリュームが Coastfile の想定と一致するようになります。

## トラブルシューティング

共有サービスが誤ったホストボリュームを参照しているように見える場合:

1. [Coastguard](COASTGUARD.md) UI（`coast ui`）を開きます。
2. **Shared Services** タブに移動します。
3. 影響を受けているサービスを選択し、**Remove** をクリックします。
4. **Refresh Shared Services** をクリックして、現在の Coastfile 設定からそれらを再作成します。

これにより、共有サービスコンテナが停止・再作成され、正しいホストボリュームに再接続されます。

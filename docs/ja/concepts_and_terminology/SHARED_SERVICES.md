# 共有サービス

共有サービスは、Coast 内ではなくホストの Docker デーモン上で動作するデータベースおよびインフラストラクチャ用コンテナ（Postgres、Redis、MongoDB など）です。Coast インスタンスはブリッジネットワーク経由でそれらに接続するため、すべての Coast は同じホストボリューム上の同じサービスと通信します。

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*ホスト管理の Postgres、Redis、MongoDB を表示している Coastguard の共有サービス タブ。*

## 仕組み

Coastfile で共有サービスを宣言すると、Coast はそれをホストのデーモン上で起動し、各 Coast コンテナ内で動作する compose スタックからは取り除きます。その後、Coast は接続先をホストへ戻すように設定されます。

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

共有サービスは既存のホストボリュームを再利用するため、ローカルで `docker-compose up` を実行して得られた既存データは、ただちに Coasts から利用できます。

## 共有サービスを使うべきとき

- ローカルデータベースに接続する MCP 統合がプロジェクトにある — 共有サービスにより、再設定なしでそれらを動かし続けられます。`localhost:5432` に接続するホスト上のデータベース MCP は、共有 Postgres が同じポートでホスト上にあるため、そのまま動作します。動的なポート検出も MCP の再設定も不要です。詳しくは [MCP Servers](MCP_SERVERS.md) を参照してください。
- Coast インスタンスを軽量にしたい（各インスタンスが独自のデータベースコンテナを実行する必要がないため）。
- Coast インスタンス間のデータ分離が不要（すべてのインスタンスが同じデータを見る）である。
- ホスト上でコーディングエージェントを実行しており（[Filesystem](FILESYSTEM.md) を参照）、[`coast exec`](EXEC_AND_DOCKER.md) を経由してルーティングせずにデータベースの状態へアクセスさせたい。共有サービスを使うと、エージェントが既に使っているデータベースツールや MCP は変更なしで動作します。

分離が必要な場合の代替案については [Volume Topology](VOLUMES.md) ページを参照してください。

## ボリュームの識別に関する警告

Docker のボリューム名は常にグローバルに一意とは限りません。複数の異なるプロジェクトから `docker-compose up` を実行している場合、Coast が共有サービスに接続するホストボリュームが、想定しているものと異なる可能性があります。

共有サービス付きで Coasts を開始する前に、最後に実行した `docker-compose up` が、Coasts で使用する意図のあるプロジェクトのものであることを確認してください。これにより、ホストボリュームが Coastfile の想定と一致します。

## トラブルシューティング

共有サービスが誤ったホストボリュームを指しているように見える場合:

1. [Coastguard](COASTGUARD.md) UI（`coast ui`）を開きます。
2. **Shared Services** タブへ移動します。
3. 影響を受けているサービスを選択し、**Remove** をクリックします。
4. **Refresh Shared Services** をクリックして、現在の Coastfile 設定から再作成します。

これにより共有サービスコンテナが停止・削除され、再作成されて、正しいホストボリュームへ再接続されます。

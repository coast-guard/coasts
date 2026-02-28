# ボリュームトポロジー

Coast は、データ量の多いサービス（データベース、キャッシュなど）が Coast インスタンス間でデータをどのように保存・共有するかを制御する 3 つのボリューム戦略を提供します。適切な戦略の選択は、どれだけの分離（アイソレーション）が必要か、そしてどれだけのオーバーヘッドを許容できるかに依存します。

## 共有サービス

[共有サービス](SHARED_SERVICES.md) は、いかなる Coast コンテナの外側、つまりホストの Docker デーモン上で動作します。Postgres、MongoDB、Redis などのサービスはホストマシン上に留まり、Coast インスタンスはブリッジネットワーク経由でホストへ呼び出しをルーティングして戻します。

```text
Host machine
  |
  +--> Postgres (host daemon, existing volume)
  +--> Redis (host daemon, existing volume)
  |
  +--> Coast: dev-1  --connects to--> host Postgres, host Redis
  +--> Coast: dev-2  --connects to--> host Postgres, host Redis
```

インスタンス間のデータ分離はありません — すべての Coast が同じデータベースと通信します。その代わり、次の利点があります:

- 自前のデータベースコンテナを起動しないため、Coast インスタンスが軽量になります。
- 既存のホストボリュームがそのまま直接再利用されるため、すでにあるデータは即座に利用できます。
- ローカルデータベースに接続する MCP 統合は、追加設定なしですぐに動作し続けます。

これは [Coastfile](COASTFILE_TYPES.md) の `[shared_services]` で設定します。

## 共有ボリューム

共有ボリュームは、すべての Coast インスタンスで共有される単一の Docker ボリュームをマウントします。サービス自体（Postgres、Redis など）は各 Coast コンテナ内で動作しますが、全員が同じ基盤となるボリュームに対して読み書きします。

```text
Coast: dev-1  --mounts--> shared volume "my-project-postgres"
Coast: dev-2  --mounts--> shared volume "my-project-postgres"
```

これにより、ホストマシン上のものから Coast のデータを分離できますが、インスタンス同士では引き続きデータを共有します。ホストの開発環境から明確に切り離したい一方で、インスタンスごとのボリュームによるオーバーヘッドは避けたい場合に有用です。

```toml
[volumes.postgres_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## 分離ボリューム

分離ボリュームは、各 Coast インスタンスに独立した専用ボリュームを提供します。インスタンス間でもホストとも、データは一切共有されません。各インスタンスは空の状態（またはスナップショットから — 下記参照）で開始し、それぞれ独立に分岐していきます。

```text
Coast: dev-1  --mounts--> volume "dev-1-postgres"
Coast: dev-2  --mounts--> volume "dev-2-postgres"
```

これは、統合テストが多く、並列環境間で真のボリューム分離が必要なプロジェクトにとって最適な選択です。トレードオフとして、各インスタンスがデータのコピーを保持するため、起動が遅くなり Coast のビルドも大きくなります。

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## スナップショット

共有および分離の両戦略は、デフォルトでは空のボリュームから開始します。既存のホストボリュームのコピーを使ってインスタンスを開始したい場合は、`snapshot_source` をコピー元の Docker ボリューム名に設定してください:

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

スナップショットは [ビルド時](BUILDS.md) に取得されます。作成後は各インスタンスのボリュームが独立に分岐し — 変更はソースにも他のインスタンスにも反映（伝播）されません。

Coast はまだランタイムでのスナップショット（例: 実行中インスタンスからのボリュームスナップショット）をサポートしていません。これは将来のリリースで予定されています。

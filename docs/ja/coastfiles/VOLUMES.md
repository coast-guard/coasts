# ボリューム

`[volumes.*]` セクションは、名前付き Docker ボリュームが Coast インスタンス間でどのように扱われるかを制御します。各ボリュームは、インスタンスがデータを共有するか、独立したコピーを持つかを決定する戦略で設定されます。

共有サービスを代替案として含む Coast におけるデータ分離の全体像については、[Volumes](../concepts_and_terminology/VOLUMES.md) を参照してください。

## ボリュームの定義

各ボリュームは `[volumes]` 配下の名前付き TOML セクションです。3 つのフィールドが必須です。

- **`strategy`** — `"isolated"` または `"shared"`
- **`service`** — このボリュームを使用する compose のサービス名
- **`mount`** — ボリュームのコンテナ内マウントパス

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## 戦略

### `isolated`

各 Coast インスタンスは独立したボリュームを持ちます。データはインスタンス間で共有されません。ボリュームは `coast run` で作成され、`coast rm` で削除されます。

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

これはほとんどのデータベース用ボリュームに適した選択です。各インスタンスはクリーンスレートから開始し、他のインスタンスに影響を与えることなく自由にデータを変更できます。

### `shared`

すべての Coast インスタンスが単一の Docker ボリュームを使用します。あるインスタンスが書き込んだデータは、他のすべてのインスタンスから参照できます。

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

共有ボリュームは `coast rm` では決して削除されません。手動で削除するまで永続します。

データベースのようなサービスにアタッチされたボリュームで `shared` を使用している場合、Coast はビルド時に警告を出力します。単一のデータベースボリュームを複数の同時実行インスタンスで共有すると、破損を引き起こす可能性があります。共有データベースが必要な場合は、代わりに [shared services](SHARED_SERVICES.md) を使用してください。

共有ボリュームの良い用途: 依存関係キャッシュ（Go modules、npm cache、pip cache）、ビルド成果物キャッシュ、そして同時書き込みが安全、または起こりにくいその他のデータ。

## スナップショットのシーディング

分離ボリュームは、インスタンス作成時に `snapshot_source` を使って既存の Docker ボリュームからシードできます。ソースボリュームのデータが新しい分離ボリュームにコピーされ、その後は独立して分岐します。

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source` は `strategy = "isolated"` の場合にのみ有効です。共有ボリュームに設定するとエラーになります。

これは、各 Coast インスタンスをホストの開発用データベースからコピーした現実的なデータセットで開始したい一方で、インスタンスがそのデータを自由に変更してもソースや他のインスタンスに影響を与えないようにしたい場合に有用です。

## 例

### 分離データベース、共有の依存関係キャッシュ

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### スナップショットでシードされたフルスタック

各インスタンスはホスト上の既存のデータベースボリュームのコピーから開始し、その後は独立して分岐します。

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

### インスタンスごとにクリーンなデータベースを持つテストランナー

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```

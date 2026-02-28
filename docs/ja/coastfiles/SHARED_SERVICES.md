# 共有サービス

`[shared_services.*]` セクションは、個々の Coast コンテナ内ではなくホストの Docker デーモン上で動作するインフラサービス（データベース、キャッシュ、メッセージブローカー）を定義します。複数の Coast インスタンスがブリッジネットワーク経由で同じ共有サービスに接続します。

共有サービスが実行時にどのように動作するか、ライフサイクル管理、トラブルシューティングについては、[Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) を参照してください。

## 共有サービスの定義

各共有サービスは `[shared_services]` 配下の名前付き TOML セクションです。`image` フィールドは必須で、それ以外はすべて任意です。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image`（必須）

ホストデーモン上で実行する Docker イメージ。

### `ports`

サービスが公開するポートのリスト。共有サービスと Coast インスタンス間のブリッジネットワークルーティングに使用されます。

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

ポート値は 0 以外でなければなりません。

### `volumes`

データ永続化のための Docker ボリュームのバインド文字列。これらはホストレベルの Docker ボリュームであり、Coast 管理のボリュームではありません。

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

サービスコンテナに渡される環境変数。

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

`true` の場合、Coast は各 Coast インスタンスごとに、共有サービス内にインスタンス専用データベースを自動作成します。デフォルトは `false` です。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

共有サービスの接続情報を、環境変数またはファイルとして Coast インスタンスに注入します。[secrets](SECRETS.md) と同じ `env:NAME` または `file:/path` 形式を使用します。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

## ライフサイクル

共有サービスは、それらを参照する最初の Coast インスタンスが実行されたときに自動的に起動します。`coast stop` や `coast rm` をまたいで動作し続けます。インスタンスを削除しても共有サービスのデータには影響しません。`coast shared rm` のみが共有サービスを停止して削除します。

`auto_create_db` によって作成されたインスタンスごとのデータベースも、インスタンス削除後に残ります。明示的に削除するには `coast shared db drop` を使用してください。

## 共有サービスとボリュームを使い分ける場合

複数の Coast インスタンスが同じデータベースサーバーに接続する必要がある場合（例:各インスタンスが独自のデータベースを持つ共有 Postgres）には共有サービスを使用してください。compose 内部サービスのデータを共有または分離する方法を制御したい場合は、[volume strategies](VOLUMES.md) を使用してください。

## 例

### Postgres、Redis、MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

### 最小構成の共有 Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### 自動作成データベース付き共有サービス

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

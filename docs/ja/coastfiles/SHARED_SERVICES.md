# 共有サービス

`[shared_services.*]` セクションは、個々の Coast コンテナ内部ではなくホストの Docker デーモン上で実行されるインフラサービス（データベース、キャッシュ、メッセージブローカー）を定義します。複数の Coast インスタンスは、ブリッジネットワーク経由で同じ共有サービスに接続します。

共有サービスが実行時にどのように動作するか、ライフサイクル管理、トラブルシューティングについては、[Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) を参照してください。

## 共有サービスの定義

各共有サービスは、`[shared_services]` 配下の名前付き TOML セクションです。`image` フィールドは必須で、それ以外はすべて任意です。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image`（必須）

ホストのデーモン上で実行する Docker イメージ。

### `ports`

サービスが公開するポートの一覧。共有サービスと Coast インスタンス間のブリッジネットワークのルーティングに使用されます。

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

ポート値は 0 以外でなければなりません。

### `volumes`

データ永続化のための Docker ボリュームのバインド文字列。これらはホストレベルの Docker ボリュームであり、Coast が管理するボリュームではありません。

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

`true` の場合、Coast は各 Coast インスタンスごとに、共有サービス内にインスタンス単位のデータベースを自動作成します。デフォルトは `false` です。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

共有サービスの接続情報を、環境変数またはファイルとして Coast インスタンスへ注入します。[secrets](SECRETS.md) と同じ `env:NAME` または `file:/path` 形式を使用します。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

## ライフサイクル

共有サービスは、それらを参照する最初の Coast インスタンスが実行されたときに自動的に開始します。`coast stop` や `coast rm` を跨いでも稼働し続けます。インスタンスを削除しても共有サービスのデータには影響しません。共有サービスを停止して削除するのは `coast shared rm` のみです。

`auto_create_db` によって作成されたインスタンス単位のデータベースも、インスタンス削除後に残ります。サービスとそのデータを完全に削除するには `coast shared-services rm` を使用してください。

## 共有サービスとボリュームの使い分け

複数の Coast インスタンスが同じデータベースサーバーに接続する必要がある場合（例:共有 Postgres を用意し、各インスタンスに専用データベースを割り当てる）は共有サービスを使用してください。compose 内部のサービスのデータを共有するか隔離するかを制御したい場合は、[ボリューム戦略](VOLUMES.md) を使用してください。

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

### データベースを自動作成する共有サービス

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

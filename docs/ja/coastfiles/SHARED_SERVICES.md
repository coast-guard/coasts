# 共有サービス

`[shared_services.*]` セクションは、Coast プロジェクトが利用するインフラサービス -- データベース、キャッシュ、メッセージブローカー -- を定義します。これには 2 つの形態があります。

- **インライン** -- `image`、`ports`、`env`、`volumes` を利用側の Coastfile に直接宣言します。Coast はホスト側コンテナを起動し、利用側アプリのトラフィックをそこへルーティングします。利用インスタンスが 1 つの個人プロジェクトや、ごく軽量なサービスに最適です。
- **Shared Service Group から (`from_group = true`)** -- サービスはそのプロジェクトの [Shared Service Group](../shared_service_groups/README.md) に存在します（`Coastfile.shared_service_groups` で宣言される別個の DinD コンテナ）。利用側の Coastfile はそれを有効化するだけです。シークレット抽出、ホスト側での canonical port への checkout、またはこのホスト上で同じ canonical port をそれぞれ必要とする複数の Coast プロジェクトを動かしたい場合に最適です（SSG は Postgres をホストの 5432 に bind せず、内部の `:5432` のまま保持するため、2 つのプロジェクトが共存できます）。

このページの後半 2 つのセクションでは、それぞれの形態を順に説明します。

共有サービスが実行時にどのように動作するか、ライフサイクル管理、トラブルシューティングについては、[Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md) を参照してください。

---

## インライン共有サービス

各インラインサービスは、`[shared_services]` 配下の名前付き TOML セクションです。`image` フィールドは必須で、それ以外はすべて任意です。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (required)

ホストのデーモン上で実行する Docker イメージ。

### `ports`

サービスが公開するポートの一覧。Coast は、コンテナポートのみの指定、または Docker Compose スタイルの `"HOST:CONTAINER"` マッピングのいずれも受け付けます。

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- `6379` のような整数のみの指定は、`"6379:6379"` の省略形です。
- `"5433:5432"` のようなマッピング文字列は、共有サービスをホストポート `5433` で公開しつつ、Coast 内部からは `service-name:5432` で到達可能なままにします。
- ホストポートとコンテナポートは、どちらも 0 以外でなければなりません。

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

### ライフサイクル

インライン共有サービスは、それらを参照する最初の Coast インスタンスが実行されたときに自動的に開始します。`coast stop` や `coast rm` を跨いでも稼働し続けます。インスタンスを削除しても共有サービスのデータには影響しません。サービスを停止して削除するのは `coast shared rm` のみです。

`auto_create_db` によって作成されたインスタンス単位のデータベースも、インスタンス削除後に残ります。サービスとそのデータを完全に削除するには `coast shared-services rm` を使用してください。

### インライン例

#### Postgres, Redis, and MongoDB

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

#### Minimal shared Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Host/container mapped Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Auto-created databases

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## Shared Service Group からの共有サービス

複数の worktree、ホスト側 checkout、SSG ネイティブなシークレット、SSG 再ビルドをまたいで維持される仮想ポートなど、構造化された共有インフラ構成を求めるプロジェクトでは、サービスを [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) に一度だけ宣言し、利用側の Coastfile から `from_group = true` で参照します。

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

TOML キー（この例では `postgres`）は、プロジェクトの `Coastfile.shared_service_groups` で宣言されたサービス名と一致している必要があります。ここで参照される SSG は **常に利用側プロジェクト自身の SSG** です（名前は `<project>-ssg` で、`<project>` は利用側の `[coast].name` です）。

### `from_group = true` で禁止されるフィールド

SSG が単一の truth source になるため、以下のフィールドは parse 時点で拒否されます。

- `image`
- `ports`
- `env`
- `volumes`

これらのいずれかを `from_group = true` と併用すると、次のようになります。

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### 利用側ごとに許可される上書き

- `inject` -- 接続文字列を公開する環境変数またはファイルパス。利用側の Coastfile ごとに、同じ SSG Postgres を異なる環境変数名で公開できます。
- `auto_create_db` -- `coast run` 時に、このサービス内にインスタンス単位のデータベースを作成するかどうか。SSG サービス自身の `auto_create_db` 値を上書きします。

### サービス未定義エラー

プロジェクトの `Coastfile.shared_service_groups` で宣言されていない名前を参照すると、`coast build` は失敗します。

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### インラインではなく `from_group` を選ぶべき場合

| Need | Inline | `from_group` |
|---|---|---|
| このホスト上で Coast プロジェクトが 1 つだけで、シークレットも不要 | どちらでも可。インラインのほうが簡単 | OK |
| **同じ** プロジェクトの複数 worktree / 利用インスタンスで 1 つの Postgres を共有したい | 動作する（兄弟は 1 つのホストコンテナを共有する） | 動作する |
| このホスト上の **異なる 2 つの Coast プロジェクト** が、それぞれ同じ canonical port を宣言する必要がある（例: 両方とも 5432 の Postgres を使いたい） | ホストポートで衝突し、同時実行できない | 必須（各プロジェクトの SSG が、ホスト 5432 を bind せずに独自の内部 Postgres を持つ） |
| `coast ssg checkout` を使ってホスト側 `psql localhost:5432` を使いたい | -- | 必須 |
| サービスに対してビルド時シークレット抽出が必要（keychain から `POSTGRES_PASSWORD` を取るなど） | -- | 必須（[SSG Secrets](../shared_service_groups/SECRETS.md) を参照） |
| 再ビルドをまたいで安定した利用側ルーティングが必要（仮想ポート） | -- | 必須（[SSG Routing](../shared_service_groups/ROUTING.md) を参照） |

SSG アーキテクチャ全体については [Shared Service Groups](../shared_service_groups/README.md) を参照してください。自動起動、drift detection、リモート利用側を含む利用側体験については [Consuming](../shared_service_groups/CONSUMING.md) を参照してください。

---

## See Also

- [Shared Services (concept)](../concepts_and_terminology/SHARED_SERVICES.md) -- 両方の形態の実行時アーキテクチャ
- [Shared Service Groups](../shared_service_groups/README.md) -- SSG コンセプトの概要
- [Coastfile: Shared Service Groups](SHARED_SERVICE_GROUPS.md) -- SSG 側 Coastfile スキーマ
- [Consuming an SSG](../shared_service_groups/CONSUMING.md) -- `from_group = true` の詳細な挙動

# 継承、タイプ、コンポジション

Coastfile は継承（`extends`）、フラグメント合成（`includes`）、アイテム削除（`[unset]`）、および compose レベルでの除外（`[omit]`）をサポートします。これらを組み合わせることで、ベース設定を一度だけ定義し、設定を重複させることなく、異なるワークフロー（テストランナー、軽量フロントエンド、スナップショット投入済みスタックなど）向けに無駄のないバリアントを作成できます。

型付き Coastfile がビルドシステムにどのように適合するかの高レベルな概要については、[Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) および [Builds](../concepts_and_terminology/BUILDS.md) を参照してください。

## Coastfile のタイプ

ベースの Coastfile は常に `Coastfile` という名前です。型付きバリアントは `Coastfile.{type}` という命名パターンを使用します:

- `Coastfile` — デフォルトタイプ
- `Coastfile.light` — タイプ `light`
- `Coastfile.snap` — タイプ `snap`
- `Coastfile.ci.minimal` — タイプ `ci.minimal`

`Coastfile.default` という名前は予約されており使用できません。末尾のドット（`Coastfile.`）も無効です。

`--type` を使って型付きバリアントを build / run します:

```
coast build --type light
coast run test-1 --type light
```

各タイプはそれぞれ独立したビルドプールを持ちます。`--type light` のビルドはデフォルトのビルドに干渉しません。

## `extends`

型付き Coastfile は、`[coast]` セクションの `extends` を使って親から継承できます。親が先に完全にパースされ、その後に子の値が上にレイヤーされます。

```toml
[coast]
extends = "Coastfile"
```

値は親 Coastfile への相対パスで、子のディレクトリを基準に解決されます。チェーンもサポートされており、子は（さらに祖父母を extends する）親を extends できます:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

循環チェーン（A extends B extends A、または A extends A）は検出され、拒否されます。

### マージのセマンティクス

子が親を extends する場合:

- **スカラーフィールド**（`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`）— 子に値があれば子が優先され、なければ親から継承されます。
- **マップ**（`[ports]`, `[egress]`）— キーでマージされます。子のキーは同名の親キーを上書きし、親のみのキーは保持されます。
- **名前付きセクション**（`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`）— 名前でマージされます。同名の子エントリは親エントリを完全に置き換え、新しい名前は追加されます。
- **`[coast.setup]`**:
  - `packages` — 重複排除した和集合（子が新しいパッケージを追加し、親のパッケージは保持されます）
  - `run` — 子のコマンドは親のコマンドの後に追加されます
  - `files` — `path` でマージ（同一パス = 子のエントリが親のエントリを置換）
- **`[inject]`** — `env` と `files` のリストは連結されます。
- **`[omit]`** — `services` と `volumes` のリストは連結されます。
- **`[assign]`** — 子に存在する場合は全体が置き換えられます（フィールドごとのマージはしません）。
- **`[agent_shell]`** — 子に存在する場合は全体が置き換えられます。

### プロジェクト名の継承

子が `name` を設定しない場合、親の名前を継承します。これは型付きバリアントとして通常の挙動であり、同一プロジェクトのバリアントであることを意味します:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — name "my-app" を継承
[coast]
extends = "Coastfile"
autostart = false
```

バリアントを別プロジェクトとして表示したい場合は、子で `name` を上書きできます:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

`includes` フィールドは、ファイル自身の値が適用される前に、1つ以上の TOML フラグメントファイルを Coastfile にマージします。これは、共有設定（例えば秘密情報のセットや MCP サーバーなど）を再利用可能なフラグメントに抽出するのに便利です。

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

インクルードされるフラグメントは、Coastfile と同じセクション構造を持つ TOML ファイルです。`[coast]` セクション（空でも可）を含む必要がありますが、フラグメント自体で `extends` や `includes` を使うことはできません。

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

`extends` と `includes` の両方が存在する場合のマージ順序:

1. 親（`extends`）を再帰的にパース
2. インクルードされた各フラグメントを順にマージ
3. ファイル自身の値を適用（これがすべてに優先）

## `[unset]`

すべてのマージが完了した後、解決済み設定から名前付きアイテムを削除します。これは、子が親から継承したものを、セクション全体を再定義せずに削除する方法です。

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

対応フィールド:

- `secrets` — 削除する secret 名のリスト
- `ports` — 削除する port 名のリスト
- `shared_services` — 削除する shared service 名のリスト
- `volumes` — 削除する volume 名のリスト
- `mcp` — 削除する MCP サーバー名のリスト
- `mcp_clients` — 削除する MCP クライアント名のリスト
- `egress` — 削除する egress 名のリスト
- `services` — 削除する（ベアな）service 名のリスト

`[unset]` は、extends + includes の完全なマージチェーンが解決された後に適用されます。最終マージ結果から、名前でアイテムを削除します。

## `[omit]`

Coast 内で実行される Docker Compose スタックから compose のサービスおよびボリュームを除外します。Coastfile レベルの設定を削除する `[unset]` とは異なり、`[omit]` は DinD コンテナ内で `docker compose up` を実行する際に、特定のサービスやボリュームを除外するよう Coast に指示します。

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — `docker compose up` から除外する compose サービス名
- **`volumes`** — 除外する compose ボリューム名

これは、`docker-compose.yml` がすべての Coast バリアントで必要ではないサービス（監視スタック、リバースプロキシ、管理ツールなど）を定義している場合に有用です。複数の compose ファイルを維持する代わりに、単一の compose ファイルを使い、バリアントごとに不要なものを取り除きます。

子が親を extends する場合、`[omit]` のリストは連結されます — 子は親の omit リストに追加します。

## 例

### 軽量テストバリアント

ベース Coastfile を extends し、autostart を無効化し、shared services を取り除き、データベースをインスタンスごとに分離して実行します:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### スナップショット投入済みバリアント

ベースから shared services を削除し、スナップショット投入済みの分離ボリュームに置き換えます:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

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

### 追加の shared services と includes を持つ型付きバリアント

ベースを extends し、MongoDB を追加し、フラグメントから追加の secrets を取り込みます:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### 多段の継承チェーン

3 段階: base -> light -> chain。

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

解決済み設定はベースの `Coastfile` から始まり、その上に `Coastfile.light` がマージされ、さらにその上に `Coastfile.chain` がマージされます。3 段階すべての Setup `run` コマンドは順序どおりに連結されます。Setup `packages` は全レベルにわたって重複排除されます。

### 大規模な compose スタックからのサービス除外

開発に不要な `docker-compose.yml` のサービスを除外します:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```

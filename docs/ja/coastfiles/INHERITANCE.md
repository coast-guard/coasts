# 継承、型、そして合成

Coastfile は、継承（`extends`）、フラグメント合成（`includes`）、項目の削除（`[unset]`）、および compose レベルでの除外（`[omit]`）をサポートします。これらを組み合わせることで、ベース設定を一度だけ定義し、設定を重複させることなく、さまざまなワークフロー向けの軽量なバリアント — テストランナー、軽量フロントエンド、スナップショットでシードされたスタック — を作成できます。

型付き Coastfile がビルドシステムにどのように適合するかについてのより高いレベルの概要は、[Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) および [Builds](../concepts_and_terminology/BUILDS.md) を参照してください。

## Coastfile の型

ベースの Coastfile は常に `Coastfile` という名前です。型付きバリアントは `Coastfile.{type}` という命名パターンを使用します。

- `Coastfile` — デフォルト型
- `Coastfile.light` — 型 `light`
- `Coastfile.snap` — 型 `snap`
- `Coastfile.ci.minimal` — 型 `ci.minimal`

任意の Coastfile には、エディタでのシンタックスハイライトのために省略可能な `.toml` 拡張子を付けることができます。型を抽出する前に `.toml` 接尾辞は取り除かれます。

- `Coastfile.toml` = `Coastfile`（デフォルト型）
- `Coastfile.light.toml` = `Coastfile.light`（型 `light`）
- `Coastfile.ci.minimal.toml` = `Coastfile.ci.minimal`（型 `ci.minimal`）

プレーン形式と `.toml` 形式の両方が存在する場合（例: `Coastfile` と `Coastfile.toml`）、`.toml` バリアントが優先されます。

`Coastfile.default` および `"toml"`（型としての）は予約済みの名前であり、使用できません。末尾のドット（`Coastfile.`）も無効です。

型付きバリアントのビルドと実行には `--type` を使用します。

```
coast build --type light
coast run test-1 --type light
```

各型はそれぞれ独立したビルドプールを持ちます。`--type light` のビルドはデフォルトのビルドに干渉しません。

## `extends`

型付き Coastfile は、`[coast]` セクション内の `extends` を使って親から継承できます。親はまず完全に解析され、その後に子の値がその上に重ねられます。

```toml
[coast]
extends = "Coastfile"
```

この値は親 Coastfile への相対パスであり、子のディレクトリを基準に解決されます。正確なパスが存在しない場合、Coast は `.toml` を付加したパスも試します。したがって、`extends = "Coastfile"` は、ディスク上に `.toml` バリアントしか存在しない場合に `Coastfile.toml` を見つけます。チェーンもサポートされており、子はさらに祖先を継承している親を継承できます。

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

循環チェーン（A が B を継承し、B が A を継承する、または A が A を継承する）は検出され、拒否されます。

### マージの意味論

子が親を継承する場合:

- **スカラー項目**（`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`）— 子に値があればそれが優先され、なければ親から継承されます。
- **マップ**（`[ports]`, `[egress]`）— キーごとにマージされます。子のキーは同名の親キーを上書きし、親にのみあるキーは保持されます。
- **名前付きセクション**（`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`）— 名前ごとにマージされます。同じ名前を持つ子エントリは親エントリを完全に置き換え、新しい名前は追加されます。
- **`[coast.setup]`**:
  - `packages` — 重複を除いた和集合（子が新しいパッケージを追加し、親のパッケージは保持される）
  - `run` — 子のコマンドは親のコマンドの後ろに追加される
  - `files` — `path` によってマージされる（同じパス = 子のエントリが親を置き換える）
- **`[inject]`** — `env` および `files` のリストは連結されます。
- **`[omit]`** — `services` および `volumes` のリストは連結されます。
- **`[assign]`** — 子に存在する場合は完全に置き換えられます（フィールド単位ではマージされません）。
- **`[agent_shell]`** — 子に存在する場合は完全に置き換えられます。

### プロジェクト名の継承

子が `name` を設定しない場合、親の名前を継承します。これは型付きバリアントでは通常の動作です — それらは同じプロジェクトのバリアントだからです。

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — 名前 "my-app" を継承
[coast]
extends = "Coastfile"
autostart = false
```

バリアントを別個のプロジェクトとして表示したい場合は、子で `name` を上書きできます。

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

`includes` フィールドは、ファイル自身の値が適用される前に、1 つ以上の TOML フラグメントファイルを Coastfile にマージします。これは、共有設定（たとえば一連の secrets や MCP サーバー）を再利用可能なフラグメントに切り出すのに便利です。

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

インクルードされるフラグメントは、Coastfile と同じセクション構造を持つ TOML ファイルです。`[coast]` セクション（空でも可）を含んでいる必要がありますが、それ自体で `extends` や `includes` を使うことはできません。

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

`extends` と `includes` の両方が存在する場合のマージ順序:

1. 親を解析する（`extends` 経由）、再帰的に
2. 各インクルードされたフラグメントを順番にマージする
3. ファイル自身の値を適用する（これがすべてに優先する）

## `[unset]`

すべてのマージが完了した後、解決済み設定から名前付き項目を削除します。これは、子が親から継承したものを、セクション全体を再定義することなく削除する方法です。

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

サポートされているフィールド:

- `secrets` — 削除する secret 名のリスト
- `ports` — 削除するポート名のリスト
- `shared_services` — 削除する共有サービス名のリスト
- `volumes` — 削除するボリューム名のリスト
- `mcp` — 削除する MCP サーバー名のリスト
- `mcp_clients` — 削除する MCP クライアント名のリスト
- `egress` — 削除する egress 名のリスト
- `services` — 削除する bare service 名のリスト

`[unset]` は、完全な extends + includes のマージチェーンが解決された後に適用されます。最終的にマージされた結果から、名前によって項目を削除します。

## `[omit]`

Coast 内で実行される Docker Compose スタックから compose のサービスとボリュームを除外します。`[unset]` が Coastfile レベルの設定を削除するのに対して、`[omit]` は DinD コンテナ内で `docker compose up` を実行する際に、特定のサービスまたはボリュームを除外するよう Coast に指示します。

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — `docker compose up` から除外する compose サービス名
- **`volumes`** — 除外する compose ボリューム名

これは、`docker-compose.yml` にすべての Coast バリアントで必要ではないサービス — 監視スタック、リバースプロキシ、管理ツール — が定義されている場合に便利です。複数の compose ファイルを維持する代わりに、1 つの compose ファイルを使い、バリアントごとに不要なものを取り除きます。

子が親を継承する場合、`[omit]` のリストは連結されます — 子は親の omit リストに追加します。

## 例

### 軽量テストバリアント

ベースの Coastfile を継承し、autostart を無効化し、共有サービスを取り除き、データベースをインスタンスごとに分離して実行します。

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

### スナップショットでシードされたバリアント

ベースから共有サービスを削除し、それらをスナップショットでシードされた分離ボリュームに置き換えます。

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

### 追加の共有サービスと includes を持つ型付きバリアント

ベースを継承し、MongoDB を追加し、フラグメントから追加の secrets を取り込みます。

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

### 多段階の継承チェーン

3 段階の深さ: base -> light -> chain。

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

解決済み設定はベースの `Coastfile` から始まり、その上に `Coastfile.light` をマージし、さらにその上に `Coastfile.chain` をマージします。3 段階すべての Setup `run` コマンドは順番に連結されます。Setup `packages` は全段階にわたって重複排除されます。

### 大規模な compose スタックからサービスを除外する

開発に不要なサービスを `docker-compose.yml` から取り除きます。

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

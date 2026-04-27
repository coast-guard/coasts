# Coastfile.shared_service_groups

`Coastfile.shared_service_groups` は、プロジェクトの Shared Service Group (SSG) が実行するサービスを宣言する型付き Coastfile です。通常の `Coastfile` と並んで配置され、プロジェクト名はその兄弟ファイル内の `[coast].name` から取得されます -- ここで繰り返し記述する必要はありません。各プロジェクトには（その worktree 内に）このファイルがちょうど 1 つあり、`<project>-ssg` コンテナがそこで宣言されたサービスを実行します。同じプロジェクト内の他の consumer Coastfile は、`[shared_services.<name>] from_group = true` を使ってこれらのサービスを参照できます。

概念、ライフサイクル、ボリューム、シークレット、および consumer 側の配線については、[Shared Service Groups documentation](../shared_service_groups/README.md) を参照してください。

## Discovery

`coast ssg build` は、`coast build` と同じルールでファイルを見つけます:

- デフォルト: 現在の作業ディレクトリで `Coastfile.shared_service_groups` または `Coastfile.shared_service_groups.toml` を探します。どちらの形式も等価ですが、両方存在する場合は `.toml` バリアントが優先されます。
- `-f <path>` / `--file <path>` は任意のファイルを指します。
- `--working-dir <dir>` は、プロジェクトルートを Coastfile の場所から切り離します。
- `--config '<toml>'` は、スクリプト化されたフロー向けにインライン TOML を受け付けます。

## Accepted Sections

受け付けられるのは `[ssg]`、`[shared_services.<name>]`、`[secrets.<name>]`、および `[unset]` のみです。その他のトップレベルキー（`[coast]`、`[ports]`、`[services]`、`[volumes]`、`[assign]`、`[omit]`、`[inject]`、...）は、パース時に拒否されます。

`[ssg] extends = "<path>"` および `[ssg] includes = ["<path>", ...]` は、構成の組み合わせのためにサポートされています。詳細は以下の [Inheritance](#inheritance) を参照してください。

## `[ssg]`

トップレベルの SSG 設定です。

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

外側の SSG DinD のコンテナランタイムです。現時点でサポートされる値は `dind` のみで、このフィールドは省略可能であり、デフォルトは `dind` です。

## `[shared_services.<name>]`

サービスごとに 1 つのブロックです。TOML キー（`postgres`、`redis`、...）が、consumer Coastfile が参照するサービス名になります。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

SSG の内側の Docker デーモン内で実行する Docker イメージです。ホストが pull できる任意の public または private イメージが受け付けられます。

### `ports`

サービスが待ち受けるコンテナポートです。**整数のみ。**

```toml
ports = [5432]
ports = [5432, 5433]
```

- `"HOST:CONTAINER"` のマッピング（`"5432:5432"`）は**拒否**されます。SSG のホスト公開は常に動的です -- ホストポートを自分で選ぶことはありません。
- 空配列（またはフィールド自体を完全に省略）も許可されます。公開ポートのないサイドカーでも問題ありません。

各ポートは、`coast ssg run` 時に外側の DinD 上で `PUBLISHED:CONTAINER` マッピングになります。ここで `PUBLISHED` は動的に割り当てられるホストポートです。安定した consumer ルーティングのために、プロジェクトごとの別個の仮想ポートも割り当てられます -- [Routing](../shared_service_groups/ROUTING.md) を参照してください。

### `env`

内側のサービスコンテナの環境にそのまま転送される、フラットな文字列マップです。

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

Env 値はビルドマニフェストには**記録されません**。`coast build` の安全性方針に合わせて、記録されるのはキーのみです。

Coastfile にハードコードしたくない値（パスワード、API トークン）については、以下で説明する `[secrets.*]` セクションを使ってください -- これはビルド時にホストから抽出し、実行時に注入します。

### `volumes`

Docker-Compose スタイルのボリューム文字列の配列です。各エントリは次のいずれかです:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # host bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # inner named volume
]
```

**Host bind mount** -- ソースが `/` で始まります。データ本体は実際のホストファイルシステム上に存在します。外側の DinD と内側のサービスの両方が、**同じホストパス文字列**を bind します。[Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan) を参照してください。

**Inner named volume** -- ソースは Docker ボリューム名（`/` なし）です。このボリュームは SSG の内側の Docker デーモン内に存在します。SSG の再起動をまたいで永続化され、ホストからは不透明です。

パース時に拒否されるもの:

- 相対パス（`./data:/...`）。
- `..` コンポーネント。
- コンテナ専用ボリューム（ソースなし）。
- 単一サービス内でのターゲット重複。

### `auto_create_db`

`true` の場合、デーモンは実行される各 consumer Coast ごとに、このサービス内に `{instance}_{project}` データベースを作成します。認識済みのデータベースイメージ（Postgres、MySQL）にのみ適用されます。デフォルトは `false` です。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

consumer Coastfile は、この値をプロジェクトごとに上書きできます -- [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db) を参照してください。

### `inject` (not allowed)

`inject` は SSG サービス定義では**無効**です。注入は consumer 側の関心事です（異なる consumer Coastfile が、同じ SSG Postgres を異なる env-var 名で公開したい場合があります）。consumer 側の `inject` の意味論については、[Coastfile: Shared Services](SHARED_SERVICES.md#inject) を参照してください。

## `[secrets.<name>]`

`Coastfile.shared_service_groups` 内の `[secrets.*]` ブロックは、`coast ssg build` 時にホスト側の認証情報を抽出し、`coast ssg run` 時にそれらを SSG の内側のサービスに注入します。スキーマは通常の Coastfile の `[secrets.*]` を反映しており（フィールド参照については [Secrets](SECRETS.md) を参照）、SSG 固有の動作は [SSG Secrets](../shared_service_groups/SECRETS.md) に記載されています。

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"

[secrets.tls_cert]
extractor = "file"
path = "/Users/me/certs/dev.pem"
inject = "file:/etc/ssl/certs/server.pem"
```

同じ extractor が利用可能です（`env`、`file`、`command`、`keychain`、カスタム `coast-extractor-<name>`）。`inject` ディレクティブは、値を env var として渡すか、SSG の内側のサービスコンテナ内のファイルとして渡すかを選択します。

デフォルトでは、SSG ネイティブなシークレットは宣言された**すべての** `[shared_services.*]` に注入されます。対象を一部に限定するには、サービス名を明示的に列挙します:

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]      # only mounted on the postgres service
```

抽出されたシークレット値は、`coast_image = "ssg:<project>"` のもとで `~/.coast/keystore.db` に暗号化して保存されます -- これは通常の Coast keystore エントリとは別の名前空間です。`coast ssg secrets clear` 動詞を含む完全なライフサイクルについては、[SSG Secrets](../shared_service_groups/SECRETS.md) を参照してください。

## Inheritance

SSG Coastfile は、通常の Coastfile と同じ `extends` / `includes` / `[unset]` の仕組みをサポートします。共通の考え方については [Coastfile Inheritance](INHERITANCE.md) を参照してください。このセクションでは SSG 固有の形を説明します。

### `[ssg] extends` -- 親 Coastfile を取り込む

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

親ファイルは子の親ディレクトリを基準に解決されます。`.toml` の優先ルールが適用されます（パーサは最初に `Coastfile.ssg-base.toml` を試し、その後プレーンな `Coastfile.ssg-base` を試します）。絶対パスも受け付けられます。

### `[ssg] includes` -- フラグメントファイルをマージする

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

フラグメントは、それを含むファイル自身より前に、順番にマージされます。フラグメントのパスは、それを含むファイルの親ディレクトリを基準に解決されます（`.toml` の優先ルールはありません -- フラグメントは通常、正確な名前で付けられるためです）。

**フラグメント自身は `extends` や `includes` を使うことはできません。** それらは自己完結している必要があります。

### Merge semantics

- **`[ssg]` のスカラー**（`runtime`） -- 子に存在する場合は子が優先され、そうでなければ継承されます。
- **`[shared_services.*]`** -- 名前ごとの置換。親と子の両方が `postgres` を定義している場合、子のエントリが親のものを完全に置き換えます（フィールド単位のマージではなく、エントリ全体の置換）。子が再宣言しない親サービスは継承されます。
- **`[secrets.*]`** -- 名前ごとの置換で、形は `[shared_services.*]` と同じです。同じ名前を持つ子のシークレットは、親のシークレット設定を完全に上書きします。
- **読み込み順** -- 最初に `extends` の親が読み込まれ、その後に各 `includes` フラグメントが順番に読み込まれ、最後にトップレベルファイル自身が読み込まれます。衝突時には後のレイヤーが優先されます。

### `[unset]` -- 継承されたサービスまたはシークレットを削除する

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
secrets = ["pg_password"]
```

名前付きエントリを**マージ後に**削除するため、子は親が提供するものを選択的に削除できます。`shared_services` と `secrets` の両方のキーがサポートされています。

スタンドアロンの SSG Coastfile に `[unset]` が技術的に含まれていても構いませんが、その場合は黙って無視されます（通常の Coastfile の動作と一致します: unset はファイルが継承に参加している場合にのみ適用されます）。

### Cycles

直接循環（`A` extends `B` extends `A`、または `A` が自分自身を extends する）は、`circular extends/includes dependency detected: '<path>'` でハードエラーになります。ダイヤモンド継承（2 つの別個のパスが同じ親に行き着くもの）は許可されます -- visit-set は再帰ごとに管理され、復帰時に pop されます。

### `[omit]` is not applicable

通常の Coastfile は、compose file からサービス / ボリュームを取り除くために `[omit]` をサポートします。SSG には取り除く compose file がありません -- `[shared_services.*]` エントリから直接、内側の compose を生成します。代わりに、継承されたサービスを削除するには `[unset]` を使ってください。

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'` は親パスを解決できません。相対パスの基準となるディスク上の場所が存在しないためです。インライン TOML で `extends` / `includes` を渡すと、`extends and includes require file-based parsing` でハードエラーになります。代わりに `-f <file>` または `--working-dir <dir>` を使ってください。

### Build artifact is the flattened form

`coast ssg build` は、スタンドアロンの TOML を `~/.coast/ssg/<project>/builds/<id>/ssg-coastfile.toml` に書き出します。この成果物には、継承後にマージされた結果が含まれ、`extends`、`includes`、`[unset]` ディレクティブは含まれません。そのため、親 / フラグメントファイルが存在しなくてもビルドを検査または再実行できます。`build_id` ハッシュもこのフラット化された形式を反映するため、親だけの変更でもキャッシュが正しく無効化されます。

## Example

env から抽出したパスワードを使う Postgres + Redis:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- 概念の概要
- [SSG Building](../shared_service_groups/BUILDING.md) -- `coast ssg build` がこのファイルに対して何を行うか
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- ボリューム宣言の形式、権限、およびホストボリューム移行レシピ
- [SSG Secrets](../shared_service_groups/SECRETS.md) -- `[secrets.*]` のビルド時抽出 / 実行時注入パイプライン
- [SSG Routing](../shared_service_groups/ROUTING.md) -- canonical / dynamic / virtual ports
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- consumer 側の `from_group = true` 構文
- [Coastfile: Secrets and Injection](SECRETS.md) -- 通常の Coastfile の `[secrets.*]` リファレンス
- [Coastfile Inheritance](INHERITANCE.md) -- 共通の `extends` / `includes` / `[unset]` の考え方

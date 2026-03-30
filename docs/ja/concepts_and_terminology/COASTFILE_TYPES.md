# Coastfile タイプ

1 つのプロジェクトは、異なるユースケースのために複数の Coastfile を持つことができます。各バリアントは「タイプ」と呼ばれます。タイプを使うと、共通のベースを共有しつつ、実行するサービス、ボリュームの扱い、サービスを自動起動するかどうかが異なる構成を組み合わせることができます。

## タイプの仕組み

命名規則は、デフォルト用が `Coastfile`、バリアント用が `Coastfile.{type}` です。ドットの後ろの接尾辞がタイプ名になります。

- `Coastfile` -- デフォルトタイプ
- `Coastfile.test` -- テストタイプ
- `Coastfile.snap` -- スナップショットタイプ
- `Coastfile.light` -- 軽量タイプ

任意の Coastfile には、エディタのシンタックスハイライト用にオプションで `.toml` 拡張子を付けることができます。タイプを導出する前に `.toml` 接尾辞は取り除かれるため、次のペアは同等です。

- `Coastfile.toml` = `Coastfile`（デフォルトタイプ）
- `Coastfile.test.toml` = `Coastfile.test`（テストタイプ）
- `Coastfile.light.toml` = `Coastfile.light`（軽量タイプ）

**競合時のルール:** 両方の形式が存在する場合（例: `Coastfile` と `Coastfile.toml`、または `Coastfile.light` と `Coastfile.light.toml`）、`.toml` バリアントが優先されます。

**予約済みのタイプ名:** `"default"` と `"toml"` はタイプ名として使用できません。`Coastfile.default` と `Coastfile.toml`（タイプ接尾辞として、つまりファイル名が文字通り `Coastfile.toml.toml` であることを意味する）は拒否されます。

型付き Coast は `--type` を使って build と run を行います。

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

型付き Coastfile は `extends` を通じて親から継承します。親の内容はすべてマージされます。子は、上書きまたは追加するものだけを指定すれば十分です。

```toml
[coast]
extends = "Coastfile"
```

これにより、各バリアントごとに構成全体を複製する必要がなくなります。子は、親からすべての [ports](PORTS.md)、[secrets](SECRETS.md)、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md)、[assign strategies](ASSIGN.md)、セットアップコマンド、および [MCP](MCP_SERVERS.md) 構成を継承します。子が定義したものは、親より優先されます。

## [unset]

親から継承された特定の項目を名前で削除します。`ports`、`shared_services`、`secrets`、`volumes` を unset できます。

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

これは、テストバリアントが共有サービスを削除し（その結果、データベースは分離されたボリュームを持つ Coast 内で実行される）、不要なポートを取り除く方法です。

## [omit]

compose サービスをビルドから完全に取り除きます。省略されたサービスは compose ファイルから削除され、Coast 内では一切実行されません。

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

これを使うと、そのバリアントの目的に関係のないサービスを除外できます。テストバリアントでは、データベース、マイグレーション、テストランナーだけを残すことがあります。

## autostart

Coast の起動時に `docker compose up` を自動実行するかどうかを制御します。デフォルトは `true` です。

```toml
[coast]
extends = "Coastfile"
autostart = false
```

完全なスタックを立ち上げるのではなく、特定のコマンドを手動で実行したいバリアントには `autostart = false` を設定します。これはテストランナーで一般的です -- Coast を作成してから、[`coast exec`](EXEC_AND_DOCKER.md) を使って個別のテストスイートを実行します。

## 一般的なパターン

### テストバリアント

テスト実行に必要なものだけを残す `Coastfile.test`:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

各テスト Coast は、それぞれ独自のクリーンなデータベースを持ちます。テストは内部 compose ネットワーク経由でサービスと通信するため、ポートは公開されません。`autostart = false` は、`coast exec` でテスト実行を手動で開始することを意味します。

### スナップショットバリアント

ホスト上の既存のデータベースボリュームのコピーで各 Coast を初期化する `Coastfile.snap`:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

共有サービスは unset されるため、データベースは各 Coast 内で実行されます。`snapshot_source` は、ビルド時に既存のホストボリュームから分離ボリュームへ初期データを投入します。作成後は、各インスタンスのデータはそれぞれ独立して分岐していきます。

### 軽量バリアント

特定のワークフローのためにプロジェクトを最小構成まで削ぎ落とす `Coastfile.light` -- たとえば、高速な反復作業のためにバックエンドサービスとそのデータベースだけにする場合です。

## 独立したビルドプール

各タイプは、それぞれ専用の `latest-{type}` シンボリックリンクと、専用の 5 ビルド自動削除プールを持ちます。

```bash
coast build              # latest を更新し、default ビルドを削除
coast build --type test  # latest-test を更新し、test ビルドを削除
coast build --type snap  # latest-snap を更新し、snap ビルドを削除
```

`test` タイプをビルドしても、`default` や `snap` のビルドには影響しません。削除処理はタイプごとに完全に独立しています。

## 型付き Coast の実行

`--type` で作成されたインスタンスには、そのタイプがタグ付けされます。同じプロジェクトに対して、異なるタイプのインスタンスを同時に実行できます。

```bash
coast run dev-1                    # default type
coast run test-1 --type test       # test type
coast run snapshot-1 --type snap   # snapshot type

coast ls
# All three appear, each with their own type, ports, and volume strategy
```

これにより、完全な開発環境を動かしながら、分離されたテストランナーやスナップショットで初期化されたインスタンスも、同じプロジェクトに対して、同時に並行して実行できます。

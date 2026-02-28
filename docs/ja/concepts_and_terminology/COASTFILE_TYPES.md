# Coastfile のタイプ

1つのプロジェクトは、異なるユースケース向けに複数の Coastfile を持てます。各バリアントは「type（タイプ）」と呼ばれます。タイプを使うと、共通のベースを共有しつつ、実行するサービス、ボリュームの扱い、サービスを自動起動するかどうかといった点が異なる設定を組み立てられます。

## Types の仕組み

命名規則は、デフォルトが `Coastfile`、バリアントが `Coastfile.{type}` です。ドットの後ろのサフィックスがタイプ名になります。

- `Coastfile` -- デフォルトタイプ
- `Coastfile.test` -- テストタイプ
- `Coastfile.snap` -- スナップショットタイプ
- `Coastfile.light` -- 軽量タイプ

タイプ付き Coast は `--type` でビルドと実行を行います。

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

タイプ付き Coastfile は `extends` により親から継承します。親の内容はすべてマージされます。子は上書きまたは追加するものだけを指定すれば十分です。

```toml
[coast]
extends = "Coastfile"
```

これにより、バリアントごとに設定全体を複製することを避けられます。子は親から、すべての [ports](PORTS.md)、[secrets](SECRETS.md)、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md)、[assign strategies](ASSIGN.md)、セットアップコマンド、そして [MCP](MCP_SERVERS.md) の設定を継承します。子で定義したものは親より優先されます。

## [unset]

親から継承した特定の項目を、名前で指定して削除します。`ports`、`shared_services`、`secrets`、`volumes` を unset できます。

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

これは、テスト用バリアントが shared services を外し（データベースを Coast 内で分離ボリューム付きで動かすため）、不要なポートを削除する方法です。

## [omit]

compose サービスをビルドから完全に取り除きます。omit されたサービスは compose ファイルから削除され、Coast 内では一切実行されません。

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

バリアントの目的に無関係なサービスを除外するために使います。テスト用バリアントでは、データベース、マイグレーション、テストランナーだけを残すことがあります。

## autostart

Coast の起動時に `docker compose up` を自動実行するかどうかを制御します。デフォルトは `true` です。

```toml
[coast]
extends = "Coastfile"
autostart = false
```

フルスタックを起動するのではなく、特定のコマンドを手動で実行したいバリアントでは `autostart = false` を設定します。これはテストランナーで一般的です。Coast を作成し、その後 [`coast exec`](EXEC_AND_DOCKER.md) を使って個別のテストスイートを実行します。

## よくあるパターン

### テスト用バリアント

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

各テスト Coast はそれぞれクリーンなデータベースを持ちます。テストは内部の compose ネットワーク越しにサービスへ接続するため、ポートは公開されません。`autostart = false` は、`coast exec` で手動でテスト実行をトリガーすることを意味します。

### スナップショット用バリアント

ホストに既存のデータベースボリュームのコピーを各 Coast に投入する `Coastfile.snap`:

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

shared services は unset され、データベースは各 Coast 内で実行されます。`snapshot_source` は、ビルド時に既存のホストボリュームから分離ボリュームへシードします。作成後は、各インスタンスのデータは独立して分岐します。

### 軽量バリアント

特定のワークフロー向けに、プロジェクトを最小構成まで削ぎ落とす `Coastfile.light` — たとえば高速な反復のために、バックエンドサービスとそのデータベースだけにする、といった用途です。

## 独立したビルドプール

各タイプはそれぞれ独自の `latest-{type}` シンボリックリンクと、5ビルドの自動プルーニングプールを持ちます。

```bash
coast build              # latest を更新し、default ビルドを prune
coast build --type test  # latest-test を更新し、test ビルドを prune
coast build --type snap  # latest-snap を更新し、snap ビルドを prune
```

`test` タイプをビルドしても `default` や `snap` のビルドには影響しません。プルーニングはタイプごとに完全に独立しています。

## タイプ付き Coast の実行

`--type` で作成されたインスタンスには、そのタイプがタグ付けされます。同一プロジェクトで、異なるタイプのインスタンスを同時に実行できます。

```bash
coast run dev-1                    # default type
coast run test-1 --type test       # test type
coast run snapshot-1 --type snap   # snapshot type

coast ls
# All three appear, each with their own type, ports, and volume strategy
```

これにより、同じプロジェクトに対して、フルの開発環境を動かしながら、分離されたテストランナーやスナップショットでシードされたインスタンスも並行して、すべて同時に運用できます。

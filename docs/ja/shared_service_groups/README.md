# Shared Service Groups

Shared Service Group（SSG）とは、プロジェクトのインフラサービス -- Postgres、Redis、MongoDB、その他通常 `[shared_services]` に置くものすべて -- を1か所で実行する Docker-in-Docker コンテナであり、それを利用する [Coast](../concepts_and_terminology/COASTS.md) インスタンスとは分離されています。すべての Coast プロジェクトは、それぞれ固有の SSG を持ちます。名前は `<project>-ssg` で、プロジェクトの `Coastfile` と同じ階層にある `Coastfile.shared_service_groups` によって定義されます。

各コンシューマインスタンス（`dev-1`, `dev-2`, ...）は、安定した仮想ポートを介して自分のプロジェクトの SSG に接続するため、SSG の再ビルドによってコンシューマが影響を受けることはありません。各 Coast 内では契約は変わりません: `postgres:5432` は共有 Postgres に解決され、アプリケーションコードは特別なことが起きているとは認識しません。

## Why an SSG

元の [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) パターンでは、ホストの Docker デーモン上で1つのインフラコンテナを起動し、それをプロジェクト内のすべてのコンシューマインスタンスで共有します。これは1つのプロジェクトであれば問題なく機能します。問題が始まるのは、**2つの異なるプロジェクト** がそれぞれ `5432` 上の Postgres を定義している場合です: 両方のプロジェクトが同じホストポートをバインドしようとするため、後から起動したほうが失敗します。

```text
Without an SSG (cross-project host-port collision):

Host Docker daemon
+-- cg-coasts-postgres            (project "cg" binds host :5432)
+-- filemap-coasts-postgres       (project "filemap" tries :5432 -- FAILS)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (siblings share fine)
```

SSG は、各プロジェクトのインフラをそれぞれ専用の DinD に持ち上げることでこれを解決します。Postgres は引き続き標準の `:5432` で待ち受けます -- ただしホスト上ではなく SSG の内部でです。SSG コンテナ自体は任意の動的ホストポートで公開され、デーモン管理の仮想ポート socat（`42000-43000` 帯）がコンシューマトラフィックをそこへ中継します。どちらもホストの 5432 をバインドしないため、2つのプロジェクトがそれぞれ標準の 5432 で Postgres を持つことができます:

```text
With an SSG (per project, no cross-project collision):

Host Docker daemon
+-- cg-ssg                        (project "cg" -- DinD)
|     +-- postgres                (inner :5432, host dyn 54201, vport 42000)
|     +-- redis                   (inner :6379, host dyn 54202, vport 42001)
+-- filemap-ssg                   (project "filemap" -- DinD, no collision)
|     +-- postgres                (inner :5432, host dyn 54250, vport 42002)
|     +-- redis                   (inner :6379, host dyn 54251, vport 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

各プロジェクトの SSG は、それぞれ独自のデータ、独自のイメージバージョン、独自のシークレットを持ちます。両者が状態を共有することはなく、ポートを奪い合うこともなく、互いのデータを見ることもありません。各コンシューマ Coast 内では契約は変わりません: アプリコードは `postgres:5432` に接続し、自分のプロジェクトの Postgres に到達します -- ルーティング層（[Routing](ROUTING.md) を参照）が残りを処理します。

## Quick Start

`Coastfile.shared_service_groups` は、プロジェクトの `Coastfile` と同じ階層に置かれます。プロジェクト名は通常の Coastfile の `[coast].name` から取得されるため、繰り返し指定する必要はありません。

```toml
# Coastfile.shared_service_groups
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_DB = "app_dev" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

# Optional: extract secrets from your environment, keychain, or 1Password
# at build time and inject them into the SSG at run time. See SECRETS.md.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

ビルドして実行します:

```bash
coast ssg build       # parse, pull images, extract secrets, write artifact
coast ssg run         # start <project>-ssg, materialize secrets, compose up
coast ssg ps          # show service status
```

コンシューマ Coast をそれに向けます:

```toml
# Coastfile in the same project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

その後 `coast build && coast run dev-1` を実行します。SSG がまだ動作していなければ自動的に起動されます。`dev-1` のアプリコンテナ内では、`postgres:5432` は SSG の Postgres に解決され、`$DATABASE_URL` には標準的な接続文字列が設定されます。

## Reference

| Page | What it covers |
|---|---|
| [Building](BUILDING.md) | `coast ssg build` のエンドツーエンド、プロジェクトごとのアーティファクトレイアウト、シークレット抽出、`Coastfile.shared_service_groups` の検出ルール、およびプロジェクトを特定のビルドに固定する方法 |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`、プロジェクトごとの `<project>-ssg` コンテナ、`coast run` 時の自動起動、およびプロジェクト横断の一覧表示のための `coast ssg ls` |
| [Routing](ROUTING.md) | 標準 / 動的 / 仮想ポート、ホストの socat レイヤー、アプリから内部サービスまでの完全なホップごとの経路、およびリモートコンシューマ向けの対称トンネル |
| [Volumes](VOLUMES.md) | ホストのバインドマウント、対称パス、内部名前付きボリューム、権限、`coast ssg doctor` コマンド、および既存のホストボリュームを SSG に移行する方法 |
| [Consuming](CONSUMING.md) | `from_group = true`、許可されるフィールドと禁止されるフィールド、競合検出、`auto_create_db`、`inject`、およびリモートコンシューマ |
| [Secrets](SECRETS.md) | SSG Coastfile における `[secrets.<name>]`、ビルド時の extractor パイプライン、`compose.override.yml` を介した実行時インジェクション、および `coast ssg secrets clear` 動詞 |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout` によって SSG の標準ポートをホスト上にバインドし、ホスト上の任意のもの（psql、redis-cli、IDE）から到達できるようにする方法 |
| [CLI](CLI.md) | すべての `coast ssg` サブコマンドの1行要約 |

## See Also

- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- SSG が一般化する、インスタンス内埋め込み型のパターン
- [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) -- `from_group` を含むコンシューマ側 TOML 構文
- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- `Coastfile.shared_service_groups` の完全なスキーマ
- [Ports](../concepts_and_terminology/PORTS.md) -- 標準ポートと動的ポートの違い

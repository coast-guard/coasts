# Shared Service Group のビルド

`coast ssg build` はプロジェクトの `Coastfile.shared_service_groups` を解析し、宣言された secret を抽出し、すべてのイメージをホストのイメージキャッシュに取り込み、`~/.coast/ssg/<project>/builds/<build_id>/` 配下にバージョン付きビルド成果物を書き込みます。このコマンドは、すでに実行中の SSG に対して破壊的ではありません。次回の `coast ssg run` または `coast ssg start` で新しいビルドが取り込まれますが、実行中の `<project>-ssg` は再起動するまで現在のビルドを提供し続けます。

プロジェクト名は兄弟の `Coastfile` にある `[coast].name` から取得されます。各プロジェクトはそれぞれ独自の `<project>-ssg` という名前の SSG、独自のビルドディレクトリ、独自の `latest_build_id` を持ちます。ホスト全体で共通の「current SSG」は存在しません。

完全な TOML スキーマについては [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) を参照してください。

## Discovery

`coast ssg build` は `coast build` と同じルールで Coastfile を見つけます:

- フラグなしでは、現在の作業ディレクトリ内の `Coastfile.shared_service_groups` または `Coastfile.shared_service_groups.toml` を探します。両方の形式は等価であり、両方存在する場合は `.toml` 接尾辞付きが優先されます。
- `-f <path>` / `--file <path>` は任意のファイルを指します。
- `--working-dir <dir>` はプロジェクトルートを Coastfile の場所から切り離します（`coast build --working-dir` と同じフラグです）。
- `--config '<inline-toml>'` は、Coastfile をインラインで生成するスクリプトや CI フローをサポートします。

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

ビルドは、同じディレクトリ内にある兄弟の `Coastfile` からプロジェクト名を解決します。`--config` を使用する場合（ディスク上に `Coastfile.shared_service_groups` がない場合）でも、cwd にはその `[coast].name` が SSG プロジェクトである `Coastfile` が存在している必要があります。

## What Build Does

各 `coast ssg build` は、`coast build` と同じ `BuildProgressEvent` チャネルを通じて進捗をストリームするため、CLI は `[N/M]` のステップカウンターを表示します。

1. **Parse** `Coastfile.shared_service_groups` を解析します。受け入れられるトップレベルセクションは `[ssg]`、`[shared_services.*]`、`[secrets.*]`、および `[unset]` です。volume エントリはホストの bind mount と内部の named volume に分割されます（[Volumes](VOLUMES.md) を参照）。
2. **Resolve the build id.** id は `{coastfile_hash}_{YYYYMMDDHHMMSS}` の形式です。hash には、生のソース、解析済みサービスの決定論的な要約、および `[secrets.*]` 設定が織り込まれます（そのため secret の `extractor` や `var` を編集すると新しい id が生成されます）。
3. **Synthesize the inner `compose.yml`.** 各 `[shared_services.*]` ブロックは、単一の Docker Compose ファイル内のエントリになります。これは、SSG の内部 Docker デーモンが `coast ssg run` 時に `docker compose up -d` で実行するファイルです。
4. **Extract secrets.** `[secrets.*]` が空でない場合、宣言された各 extractor を実行し、暗号化された結果を `coast_image = "ssg:<project>"` の下で `~/.coast/keystore.db` に保存します。Coastfile に `[secrets]` ブロックがない場合は静かにスキップされます。完全なパイプラインについては [Secrets](SECRETS.md) を参照してください。
5. **Pull and cache each image.** イメージは `~/.coast/image-cache/` に OCI tarball として保存されます。これは `coast build` も使用する同じプールです。どちらのコマンドからのキャッシュヒットも、もう一方を高速化します。
6. **Write the build artifact** を `~/.coast/ssg/<project>/builds/<build_id>/` に書き込みます。ファイルは `manifest.json`、`ssg-coastfile.toml`、`compose.yml` の 3 つです（レイアウトは以下を参照）。
7. **Update the project's `latest_build_id`.** これはファイルシステムの symlink ではなく、state database のフラグです。`coast ssg run` と `coast ssg ps` は、どのビルドを操作するかを知るためにこれを読み取ります。
8. **Auto-prune** 古いビルドをこのプロジェクトの最新 5 件までに自動 pruning します。`~/.coast/ssg/<project>/builds/` 配下のそれ以前の成果物ディレクトリはディスクから削除されます。pin されたビルド（下の「Locking a project to a specific build」を参照）は常に保持されます。

## Artifact Layout

```text
~/.coast/
  keystore.db                                          (shared, namespaced by coast_image)
  keystore.key
  image-cache/                                         (shared OCI tarball pool)
  ssg/
    cg/                                                (project "cg")
      builds/
        b455787d95cfdeb_20260420061903/                (the new build)
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               (prior build)
          ...
    filemap/                                           (project "filemap" -- separate tree)
      builds/
        ...
    runs/
      cg/                                              (per-project run scratch)
        compose.override.yml                           (rendered at coast ssg run)
        secrets/<basename>                             (file-injected secrets, mode 0600)
```

`manifest.json` は、下流コードが重要視するビルドメタデータを保持します:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

env の値と secret のペイロードは意図的に含まれていません。記録されるのは env 変数名と inject *targets* のみです。secret の値は成果物ファイルには決して含まれず、keystore に暗号化されて保存されます。

`ssg-coastfile.toml` は、解析済み・補間済み・検証後の Coastfile です。バイト単位で、デーモンが解析時に見たものと同一です。過去のビルドを監査するのに有用です。

`compose.yml` は、SSG の内部 Docker デーモンが実行するものです。合成ルール、特に対称パス bind mount 戦略については [Volumes](VOLUMES.md) を参照してください。

## Inspecting a Build Without Running It

`coast ssg ps` は、プロジェクトの `latest_build_id` に対する `manifest.json` を直接読み取ります。container は一切調べません。次の `coast ssg run` で起動されるサービスを確認するために、`coast ssg build` の直後に実行できます:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

`PORT` 列は内部コンテナポートです。動的なホストポートは `coast ssg run` 時に割り当てられます。consumer 向けの virtual port は `coast ssg ports` によって報告されます。全体像については [Routing](ROUTING.md) を参照してください。

プロジェクトのすべてのビルドを参照するには（タイムスタンプ、サービス数、どのビルドが現在 latest かを含む）、次を使用します:

```bash
coast ssg builds-ls
```

## Rebuilds

新しい `coast ssg build` は、SSG を更新するための正規の方法です。これにより secret は再抽出され（存在する場合）、`latest_build_id` が更新され、古い成果物が pruning されます。consumer は自動では再ビルドされません。consumer の `from_group = true` 参照は、その時点で current だったビルドに対して consumer-build 時に解決されるためです。consumer をより新しい SSG に切り替えるには、consumer に対して `coast build` を実行してください。

ランタイムはリビルドをまたいでも寛容です。virtual port は `(project, service, container_port)` ごとに安定しているため、routing のために consumer を更新する必要はありません。形状の変更（サービス名が変更または削除されたなど）は、Coast レベルの「drift」メッセージとしてではなく、consumer レベルの接続エラーとして現れます。その理由については [Routing](ROUTING.md) を参照してください。

## Locking a project to a specific build

デフォルトでは、SSG はプロジェクトの `latest_build_id` を実行します。以前のビルドでプロジェクトを固定したい場合 -- 回帰の再現、複数 worktree 間での 2 つのビルドの A/B 比較、または既知の良好な形状で長寿命ブランチを維持する場合 -- pin コマンドを使用します:

```bash
coast ssg checkout-build <build_id>     # このプロジェクトを <build_id> に pin する
coast ssg show-pin                      # アクティブな pin を表示する（存在する場合）
coast ssg uncheckout-build              # pin を解除する; latest に戻る
```

pin は consumer project ごとです（1 プロジェクトにつき 1 つの pin で、worktree 間で共有されます）。pin されている場合:

- `coast ssg run` は `latest_build_id` の代わりに pin されたビルドを自動起動します。
- `coast build` は `from_group` 参照を pin されたビルドの manifest に対して検証します。
- `auto_prune` は、その pin されたビルドディレクトリが最新 5 件のウィンドウ外にあっても削除しません。

Coastguard SPA は、pin がアクティブなときは build id の横に `PINNED` バッジを表示し、そうでないときは `LATEST` を表示します。pin コマンドは [CLI](CLI.md) にも記載されています。

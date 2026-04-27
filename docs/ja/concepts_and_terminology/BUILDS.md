# ビルド

coast のビルドは、追加の支援機能が付いた Docker イメージのようなものだと考えてください。ビルドはディレクトリベースの成果物であり、Coast インスタンスを作成するために必要なすべてをまとめています: 解決済みの [Coastfile](COASTFILE_TYPES.md)、書き換えられた compose ファイル、事前に pull 済みの OCI イメージ tarball、そして注入されたホストファイルです。これは Docker イメージそのものではありませんが、Docker イメージ（tarball として）と、それらを連携させるために Coast が必要とするメタデータを含んでいます。

## `coast build` が行うこと

`coast build` を実行すると、デーモンは次の手順を順番に実行します:

1. Coastfile を解析し、検証します。
2. compose ファイルを読み込み、省略されたサービスを除外します。
3. 設定された extractor から [secrets](SECRETS.md) を抽出し、暗号化して keystore に保存します。
4. `build:` ディレクティブを持つ compose サービスの Docker イメージを（ホスト上で）ビルドします。
5. `image:` ディレクティブを持つ compose サービスの Docker イメージを pull します。
6. すべてのイメージを OCI tarball として `~/.coast/image-cache/` にキャッシュします。
7. `[coast.setup]` が設定されている場合、指定されたパッケージ、コマンド、ファイルを含むカスタム DinD ベースイメージをビルドします。
8. manifest、解決済み coastfile、書き換え済み compose、注入ファイルを含むビルド成果物ディレクトリを書き出します。
9. `latest` シンボリックリンクを新しいビルドに向けて更新します。
10. 保持上限を超えた古いビルドを自動的に prune します。

## Coastfile なしのビルド

Coastfile を使わずに、設定を CLI フラグとして直接渡してプロジェクトをビルドできます:

```bash
coast build --name my-project --compose ./docker-compose.yml
```

Coastfile が存在しない場合に必要なフラグ:
- `--name <NAME>` -- プロジェクト名
- `--compose <PATH>` -- docker-compose ファイルへのパス

一般的な設定用の追加フラグ:
- `--port NAME=PORT` -- ポートマッピング（繰り返し指定可能）
- `--runtime <dind|sysbox|podman>` -- コンテナランタイム
- `--no-autostart` -- compose サービスの自動起動を無効化
- `--primary-port <NAME>` -- プライマリポートのサービス名

より複雑な設定（secrets、volumes、共有サービス）には、インライン TOML とともに `--config` を使用します:

```bash
coast build --name my-project --compose ./dc.yml \
  --port web=3000 \
  --config '[secrets.api_key]
extractor = "env"
var = "MY_API_KEY"
inject = "env:API_KEY"'
```

### Coastfile の上書き

ディスク上に Coastfile が存在する場合、CLI フラグはその値を上書きします。Coastfile はベース設定を提供し、フラグが優先されます:

```bash
coast build --name custom-name --port api=9090
```

これは既存の Coastfile を読み込みますが、`coast.name` を `custom-name` に置き換え、`api` ポートを追加（または上書き）します。

## ビルドの保存場所

```text
~/.coast/
  images/
    my-project/
      latest -> a3c7d783_20260227143000       (symlink)
      a3c7d783_20260227143000/                (versioned build)
        manifest.json
        coastfile.toml
        compose.yml
        inject/
      b4d8e894_20260226120000/                (older build)
        ...
  image-cache/                                (shared tarball cache)
    postgres_16_a1b2c3d4e5f6.tar
    redis_7_f6e5d4c3b2a1.tar
    coast-built_my-project_web_latest_...tar
```

各ビルドには、`{coastfile_hash}_{YYYYMMDDHHMMSS}` 形式の一意な **build ID** が割り当てられます。このハッシュには Coastfile の内容と解決済み設定が含まれるため、Coastfile を変更すると新しい build ID が生成されます。

`latest` シンボリックリンクは、素早く解決できるよう常に最新のビルドを指します。プロジェクトで型付き Coastfile（例: `Coastfile.light`）を使用している場合、各 type は独自のシンボリックリンクを持ちます: `latest-light`。

`~/.coast/image-cache/` にあるイメージキャッシュは、すべてのプロジェクト間で共有されます。2 つのプロジェクトが同じ Postgres イメージを使用している場合、その tarball は 1 回だけキャッシュされます。

## ビルドに含まれるもの

各ビルドディレクトリには次のものが含まれます:

- **`manifest.json`** -- 完全なビルドメタデータ: プロジェクト名、ビルドタイムスタンプ、coastfile ハッシュ、キャッシュ済み/ビルド済みイメージの一覧、シークレット名、省略されたサービス、[volume strategies](VOLUMES.md) など。
- **`coastfile.toml`** -- 解決済み Coastfile（`extends` を使用している場合は親とマージ済み）。
- **`compose.yml`** -- あなたの compose ファイルを書き換えたバージョンで、`build:` ディレクティブは事前ビルド済みのイメージタグに置き換えられ、省略されたサービスは削除されています。
- **`inject/`** -- `[inject].files` にあるホストファイル（例: `~/.gitconfig`、`~/.npmrc`）のコピー。

## ビルドにはシークレットは含まれない

シークレットはビルドステップ中に抽出されますが、ビルド成果物ディレクトリ内ではなく、`~/.coast/keystore.db` にある別の暗号化 keystore に保存されます。manifest に記録されるのは、抽出されたシークレットの **名前** のみであり、値が記録されることはありません。

これは、機密データを露出することなくビルド成果物を安全に確認できることを意味します。シークレットはその後、`coast run` で Coast インスタンスが作成される際に復号され、注入されます。

## ビルドと Docker

ビルドには 3 種類の Docker イメージが関わります:

- **Built images** -- `build:` ディレクティブを持つ compose サービスは、ホスト上で `docker build` によってビルドされ、`coast-built/{project}/{service}:latest` としてタグ付けされ、イメージキャッシュに tarball として保存されます。
- **Pulled images** -- `image:` ディレクティブを持つ compose サービスは pull され、tarball として保存されます。
- **Coast image** -- `[coast.setup]` が設定されている場合、指定されたパッケージ、コマンド、ファイルを含むカスタム Docker イメージが `docker:dind` の上にビルドされます。`coast-image/{project}:{build_id}` としてタグ付けされます。

実行時（[`coast run`](RUN.md)）には、これらの tarball は `docker load` によって内部の [DinD daemon](RUNTIMES_AND_SERVICES.md) に読み込まれます。これにより、レジストリからイメージを pull する必要なく、Coast インスタンスを高速に起動できます。

## ビルドとインスタンス

[`coast run`](RUN.md) を実行すると、Coast は最新のビルド（または特定の `--build-id`）を解決し、その成果物を使用してインスタンスを作成します。build ID はインスタンスに記録されます。

さらにインスタンスを作成するために再ビルドする必要はありません。1 つのビルドで、並行して実行される複数の Coast インスタンスに対応できます。

## 再ビルドが必要なとき

再ビルドが必要なのは、Coastfile、`docker-compose.yml`、またはインフラ設定が変更されたときだけです。再ビルドはリソースを多く消費します -- イメージの再 pull、Docker イメージの再ビルド、シークレットの再抽出が行われます。

コード変更に再ビルドは不要です。Coast はプロジェクトディレクトリを各インスタンスに直接マウントするため、コード更新は即座に反映されます。

## 自動 Pruning

Coast は Coastfile type ごとに最大 5 つのビルドを保持します。`coast build` が成功するたびに、上限を超えた古いビルドは自動的に削除されます。

実行中のインスタンスで使用されているビルドは、上限に関係なく決して prune されません。7 つのビルドがあり、そのうち 3 つがアクティブなインスタンスを支えている場合、その 3 つはすべて保護されます。

## 手動削除

ビルドは `coast rm-build` または Coastguard の Builds タブから手動で削除できます。

- **プロジェクト全体の削除** (`coast rm-build <project>`) では、最初にすべてのインスタンスを停止して削除しておく必要があります。これにより、ビルドディレクトリ全体、関連する Docker イメージ、volume、container が削除されます。
- **選択的削除**（Coastguard UI で利用可能な build ID による削除）では、実行中のインスタンスで使用中のビルドはスキップされます。

## 型付きビルド

プロジェクトで複数の Coastfile（例: デフォルト設定用の `Coastfile` と、snapshot で seed された volume 用の `Coastfile.snap`）を使用している場合、各 type は独自の `latest-{type}` シンボリックリンクと独自の 5 ビルド pruning プールを維持します。

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

`snap` ビルドの pruning が `default` ビルドに影響することはなく、その逆も同様です。

## カスタム作業ディレクトリ

デフォルトでは、`coast build` は Coastfile の親ディレクトリにプロジェクトを登録します。`--working-dir` フラグはこれを上書きし、ビルドの登録済みプロジェクトルートを Coastfile の場所から切り離します:

```bash
coast --working-dir /home/user/my-project build -f /ci/configs/Coastfile
```

これは `/ci/configs/Coastfile` にある Coastfile を使ってビルドしますが、プロジェクトルートは `/home/user/my-project` として登録します。manifest に保存される `project_root` によって `coast lookup` がインスタンスをどこで照合するかが決まるため、`/home/user/my-project` から `coast lookup` を実行すると、このビルドのインスタンスが見つかります。

`--working-dir` には相対パスまたは絶対パスを指定できます。相対パスは現在のディレクトリを基準に解決されます。

これは、CI パイプライン、モノレポ構成、あるいは Coastfile がプロジェクトソースとは別のディレクトリにあるあらゆるシナリオで役立ちます。

## リモートビルド

[remote coast](REMOTES.md) 向けにビルドする場合、ビルドは `coast-service` を通じてリモートマシン上で実行されるため、イメージはリモートのネイティブアーキテクチャを使用します。その後、成果物は再利用のためにローカルマシンへ転送されます。リモートビルドは独自の `latest-remote` シンボリックリンクを維持し、アーキテクチャごとに prune されます。詳細は [Remotes](REMOTES.md) を参照してください。

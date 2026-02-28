# ビルド

Coast のビルドは、追加の助けが付いた Docker イメージだと考えてください。ビルドはディレクトリベースの成果物で、Coast インスタンスを作成するために必要なものをすべて同梱します。解決済みの [Coastfile](COASTFILE_TYPES.md)、書き換えられた compose ファイル、事前にプルされた OCI イメージ tarball、注入されたホストファイルです。ビルド自体は Docker イメージではありませんが、Docker イメージ（tarball として）と、それらを連携させるために Coast が必要とするメタデータを含みます。

## What `coast build` Does

`coast build` を実行すると、デーモンは次の手順を順に実行します:

1. Coastfile を解析して検証します。
2. compose ファイルを読み込み、省略されたサービスをフィルタリングして除外します。
3. 設定された extractor から [secrets](SECRETS.md) を抽出し、キーストアに暗号化して保存します。
4. `build:` ディレクティブを持つ compose サービスの Docker イメージを（ホスト上で）ビルドします。
5. `image:` ディレクティブを持つ compose サービスの Docker イメージをプルします。
6. すべてのイメージを `~/.coast/image-cache/` に OCI tarball としてキャッシュします。
7. `[coast.setup]` が設定されている場合、指定されたパッケージ、コマンド、ファイルを含むカスタム DinD ベースイメージをビルドします。
8. マニフェスト、解決済み coastfile、書き換えた compose、注入ファイルを含むビルド成果物ディレクトリを書き出します。
9. `latest` シンボリックリンクを更新して新しいビルドを指すようにします。
10. 保持上限を超えた古いビルドを自動でプルーニングします。

## Where Builds Live

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

各ビルドには `{coastfile_hash}_{YYYYMMDDHHMMSS}` という形式の一意な **build ID** が割り当てられます。このハッシュには Coastfile の内容と解決済みの設定が含まれるため、Coastfile を変更すると新しい build ID が生成されます。

`latest` シンボリックリンクは、迅速に解決できるよう常に最新のビルドを指します。プロジェクトが型付き Coastfile（例: `Coastfile.light`）を使用する場合、各タイプごとにシンボリックリンクが作られます: `latest-light`。

`~/.coast/image-cache/` のイメージキャッシュは全プロジェクトで共有されます。2 つのプロジェクトが同じ Postgres イメージを使う場合、tarball は 1 回だけキャッシュされます。

## What a Build Contains

各ビルドディレクトリには次が含まれます:

- **`manifest.json`** -- 完全なビルドメタデータ: プロジェクト名、ビルド時刻、coastfile ハッシュ、キャッシュ/ビルドされたイメージ一覧、シークレット名、省略されたサービス、[ボリューム戦略](VOLUMES.md) など。
- **`coastfile.toml`** -- 解決済み Coastfile（`extends` を使用している場合は親とマージ済み）。
- **`compose.yml`** -- compose ファイルの書き換え版。`build:` ディレクティブが事前ビルド済みイメージタグに置き換えられ、省略されたサービスは取り除かれます。
- **`inject/`** -- `[inject].files` で指定したホストファイルのコピー（例: `~/.gitconfig`, `~/.npmrc`）。

## Builds Do Not Contain Secrets

シークレットはビルド手順中に抽出されますが、ビルド成果物ディレクトリの中ではなく、`~/.coast/keystore.db` にある別の暗号化キーストアに保存されます。マニフェストには抽出されたシークレットの **名前** だけが記録され、値は決して記録されません。

このため、ビルド成果物は機密データを露出させることなく安全に確認できます。シークレットは後で `coast run` で Coast インスタンスを作成する際に復号され、注入されます。

## Builds and Docker

ビルドには 3 種類の Docker イメージが関係します:

- **Built images** -- `build:` ディレクティブを持つ compose サービスはホスト上で `docker build` によりビルドされ、`coast-built/{project}/{service}:latest` としてタグ付けされ、イメージキャッシュに tarball として保存されます。
- **Pulled images** -- `image:` ディレクティブを持つ compose サービスはプルされ、tarball として保存されます。
- **Coast image** -- `[coast.setup]` が設定されている場合、指定されたパッケージ、コマンド、ファイルを含むカスタム Docker イメージが `docker:dind` の上にビルドされます。`coast-image/{project}:{build_id}` としてタグ付けされます。

実行時（`coast run`）には、これらの tarball は `docker load` により内側の [DinD daemon](RUNTIMES_AND_SERVICES.md) に読み込まれます。これにより、レジストリからイメージをプルする必要がなくなり、Coast インスタンスを素早く起動できます。

## Builds and Instances

`coast run` を実行すると、Coast は最新のビルド（または特定の `--build-id`）を解決し、その成果物を使ってインスタンスを作成します。build ID はインスタンスに記録されます。

より多くのインスタンスを作成するために再ビルドする必要はありません。1 つのビルドを、並行して動作する多くの Coast インスタンスで共有できます。

## When to Rebuild

Coastfile、`docker-compose.yml`、またはインフラ設定が変更されたときにだけ再ビルドしてください。再ビルドはリソース消費が大きく、イメージの再プル、Docker イメージの再ビルド、シークレットの再抽出を行います。

コード変更は再ビルドを必要としません。Coast はプロジェクトディレクトリを各インスタンスに直接マウントするため、コード更新は即座に反映されます。

## Auto-Pruning

Coast は Coastfile タイプごとに最大 5 つのビルドを保持します。`coast build` が成功するたびに、上限を超えた古いビルドは自動的に削除されます。

実行中インスタンスが使用しているビルドは、上限に関係なく決してプルーニングされません。7 つのビルドがあり、そのうち 3 つが稼働中インスタンスを支えている場合、その 3 つはすべて保護されます。

## Manual Removal

ビルドは `coast rm-build` で手動削除するか、Coastguard の Builds タブから削除できます。

- **Full project removal**（`coast rm-build <project>`）は、まずすべてのインスタンスを停止して削除する必要があります。ビルドディレクトリ全体と、関連する Docker イメージ、ボリューム、コンテナを削除します。
- **Selective removal**（build ID 指定、Coastguard UI で利用可能）は、稼働中インスタンスが使用しているビルドをスキップします。

## Typed Builds

プロジェクトが複数の Coastfile（例: デフォルト設定用の `Coastfile` と、スナップショットで初期化されたボリューム用の `Coastfile.snap`）を使用する場合、各タイプはそれぞれ独自の `latest-{type}` シンボリックリンクと、独自の 5 ビルドのプルーニングプールを維持します。

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

`snap` ビルドのプルーニングが `default` ビルドに触れることはなく、その逆も同様です。

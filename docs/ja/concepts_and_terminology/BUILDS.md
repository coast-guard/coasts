# ビルド

coast のビルドは、追加の支援機能が付いた Docker イメージだと考えてください。ビルドはディレクトリベースの成果物で、Coast インスタンスを作成するために必要なものをすべてまとめています: 解決済みの [Coastfile](COASTFILE_TYPES.md)、書き換えられた compose ファイル、事前に pull された OCI イメージ tarball、そして注入されたホストファイルです。これは Docker イメージそのものではありませんが、Docker イメージ（tarball として）と、それらをつなぎ合わせるために Coast が必要とするメタデータを含んでいます。

## `coast build` が行うこと

`coast build` を実行すると、デーモンは次の手順を順番に実行します:

1. Coastfile を解析して検証します。
2. compose ファイルを読み取り、省略されたサービスを除外します。
3. 設定された extractor から [secrets](SECRETS.md) を抽出し、keystore に暗号化して保存します。
4. `build:` ディレクティブを持つ compose サービスの Docker イメージを（ホスト上で）ビルドします。
5. `image:` ディレクティブを持つ compose サービスの Docker イメージを pull します。
6. すべてのイメージを `~/.coast/image-cache/` に OCI tarball としてキャッシュします。
7. `[coast.setup]` が設定されている場合、指定されたパッケージ、コマンド、ファイルを使ってカスタム DinD ベースイメージをビルドします。
8. manifest、解決済み coastfile、書き換え済み compose、注入ファイルを含むビルド成果物ディレクトリを書き込みます。
9. `latest` シンボリックリンクを更新して新しいビルドを指すようにします。
10. 保持上限を超えた古いビルドを自動的に削除します。

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

各ビルドには、`{coastfile_hash}_{YYYYMMDDHHMMSS}` 形式の一意な **build ID** が付与されます。このハッシュには Coastfile の内容と解決済み設定が含まれるため、Coastfile を変更すると新しい build ID が生成されます。

`latest` シンボリックリンクは、素早く解決できるよう常に最新のビルドを指します。プロジェクトで型付き Coastfile（例: `Coastfile.light`）を使用している場合、各タイプは独自のシンボリックリンクを持ちます: `latest-light`。

`~/.coast/image-cache/` にあるイメージキャッシュは、すべてのプロジェクトで共有されます。2 つのプロジェクトが同じ Postgres イメージを使っている場合、その tarball は 1 回だけキャッシュされます。

## ビルドに含まれるもの

各ビルドディレクトリには次のものが含まれます:

- **`manifest.json`** -- プロジェクト名、ビルドタイムスタンプ、coastfile ハッシュ、キャッシュ済み/ビルド済みイメージの一覧、シークレット名、省略されたサービス、[volume strategies](VOLUMES.md) などを含む完全なビルドメタデータ。
- **`coastfile.toml`** -- 解決済み Coastfile（`extends` を使用している場合は親とマージ済み）。
- **`compose.yml`** -- compose ファイルを書き換えたバージョンで、`build:` ディレクティブは事前ビルド済みイメージタグに置き換えられ、省略されたサービスは取り除かれます。
- **`inject/`** -- `[inject].files` からのホストファイルのコピー（例: `~/.gitconfig`、`~/.npmrc`）。

## ビルドにはシークレットは含まれません

シークレットはビルド手順中に抽出されますが、`~/.coast/keystore.db` にある別の暗号化された keystore に保存されます -- ビルド成果物ディレクトリの中には保存されません。manifest には、抽出されたシークレットの **名前** だけが記録され、値は決して記録されません。

これは、機密データを露出することなくビルド成果物を安全に確認できることを意味します。シークレットは後で、`coast run` で Coast インスタンスが作成されるときに復号化されて注入されます。

## ビルドと Docker

ビルドには 3 種類の Docker イメージが関わります:

- **ビルド済みイメージ** -- `build:` ディレクティブを持つ compose サービスは、ホスト上で `docker build` によりビルドされ、`coast-built/{project}/{service}:latest` としてタグ付けされ、イメージキャッシュに tarball として保存されます。
- **pull 済みイメージ** -- `image:` ディレクティブを持つ compose サービスは pull され、tarball として保存されます。
- **Coast イメージ** -- `[coast.setup]` が設定されている場合、指定されたパッケージ、コマンド、ファイルを使って `docker:dind` の上にカスタム Docker イメージがビルドされます。`coast-image/{project}:{build_id}` としてタグ付けされます。

ランタイム時（[`coast run`](RUN.md)）には、これらの tarball は `docker load` によって内部の [DinD daemon](RUNTIMES_AND_SERVICES.md) に読み込まれます。これにより、Coast インスタンスはレジストリからイメージを pull する必要なく高速に起動できます。

## ビルドとインスタンス

[`coast run`](RUN.md) を実行すると、Coast は最新のビルド（または特定の `--build-id`）を解決し、その成果物を使ってインスタンスを作成します。build ID はそのインスタンスに記録されます。

さらにインスタンスを作成するために再ビルドする必要はありません。1 つのビルドで、並列実行される複数の Coast インスタンスに対応できます。

## 再ビルドするタイミング

Coastfile、`docker-compose.yml`、またはインフラ構成が変更されたときだけ再ビルドしてください。再ビルドはリソース集約的です -- イメージを再 pull し、Docker イメージを再ビルドし、シークレットを再抽出します。

コードの変更では再ビルドは必要ありません。Coast はプロジェクトディレクトリを各インスタンスに直接マウントするため、コードの更新は即座に反映されます。

## 自動削除

Coast は Coastfile のタイプごとに最大 5 個のビルドを保持します。`coast build` が成功するたびに、上限を超えた古いビルドは自動的に削除されます。

実行中のインスタンスで使用されているビルドは、上限に関係なく決して削除されません。7 個のビルドがあり、そのうち 3 個がアクティブなインスタンスを支えている場合、その 3 個はすべて保護されます。

## 手動削除

ビルドは `coast rm-build` または Coastguard の Builds タブから手動で削除できます。

- **プロジェクト全体の削除** (`coast rm-build <project>`) は、まずすべてのインスタンスを停止して削除しておく必要があります。これはビルドディレクトリ全体、関連する Docker イメージ、volume、container を削除します。
- **選択的削除**（build ID による削除。Coastguard UI で利用可能）は、実行中のインスタンスで使用中のビルドをスキップします。

## 型付きビルド

プロジェクトが複数の Coastfile を使用している場合（例: デフォルト設定用の `Coastfile` と、スナップショットで seed された volume 用の `Coastfile.snap`）、各タイプは独自の `latest-{type}` シンボリックリンクと独自の 5 ビルド削除プールを維持します。

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

`snap` ビルドの削除が `default` ビルドに影響することはなく、その逆も同様です。

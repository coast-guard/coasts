# SSG ボリューム

`[shared_services.<name>]` の中では、`volumes` 配列は標準の Docker Compose 構文を使用します:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

先頭の `/` は **host bind path** を意味します -- バイト列はホストのファイルシステム上に存在し、内部サービスはそれらをその場で読み書きします。先頭のスラッシュがない場合、たとえば `pg_wal:/var/lib/postgresql/wal` では、ソースは **SSG のネストされた Docker デーモン内に存在する Docker named volume** です -- これは `coast ssg rm` を実行しても残り、`coast ssg rm --with-data` によって削除されます。どちらの形式も受け付けられます。

パース時に拒否されるもの: 相対パス (`./data:/...`)、`..` コンポーネント、コンテナ専用ボリューム（ソースなし）、および 1 つのサービス内での重複ターゲット。

## docker-compose またはインライン shared service の Docker volume を再利用する

`docker-compose up`、インラインの `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]`、または手動の `docker volume create` によって、すでにホスト Docker named volume の中にデータがある場合、ボリュームの基になるホストディレクトリを bind-mount することで、SSG に同じバイト列を読ませることができます:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

左側は既存の Docker volume のホストファイルシステム上のパスです; `docker volume inspect <name>` はこれを `Mountpoint` フィールドとして報告します。Coast はバイト列をコピーしません -- SSG は docker-compose が読み書きしていたのと同じファイルを読み書きします。`coast ssg rm`（`--with-data` なし）はその volume に触れないため、docker-compose 側も引き続きそれを使用できます。

> **なぜ単に `infra_postgres_data:/var/lib/postgresql/data` ではだめなのですか?** これはインラインの `[shared_services.*]` では動作します（volume はホスト Docker デーモン上に作成され、docker-compose から見えます）。しかし、SSG の中では同じようには動作しません -- 先頭のスラッシュのない名前は、ホストから隔離された、SSG のネストされた Docker デーモン内に新しい volume を作成します。ホストデーモン上で動作する他のものとデータを共有したい場合は、代わりに volume の mountpoint パスを使用してください。

### `coast ssg import-host-volume`

`coast ssg import-host-volume` は `docker volume inspect` によって volume の `Mountpoint` を解決し、対応する `volumes` 行を出力（または適用）するため、`/var/lib/docker/volumes/<name>/_data` パスを手で組み立てる必要がありません。

スニペットモード（デフォルト）は、貼り付け用の TOML 断片を出力します:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

出力は `[shared_services.postgres]` ブロックで、新しい `volumes = [...]` エントリがすでにマージされた状態になっています:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

適用モードは `Coastfile.shared_service_groups` をその場で書き換え、元の内容を `Coastfile.shared_service_groups.bak` に保存します:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

フラグ:

- `<VOLUME>`（位置引数） -- ホスト Docker named volume。すでに存在している必要があります（確認は `docker volume inspect` で行われます）; そうでない場合は、先に `docker volume create` で作成または名前変更してください。
- `--service` -- 編集する `[shared_services.<name>]` セクション。セクションはすでに存在している必要があります。
- `--mount` -- 絶対コンテナパス。相対パスは拒否されます。同じサービス上の重複 mount パスは即時エラーになります。
- `--file` / `--working-dir` / `--config` -- SSG Coastfile 検出。ルールは `coast ssg build` と同じです。
- `--apply` -- Coastfile をその場で書き換えます。`--config` と組み合わせることはできません（インラインテキストには書き戻す先がないためです）。

`.bak` ファイルには元のバイト列がそのまま入っているため、適用前の正確な状態を復元できます。

`/var/lib/docker/volumes/<name>/_data` は、Docker が長年 volume mountpoint として使用してきたパスであり、現在 `docker volume inspect` が報告するものです。Docker はこのパスを将来にわたって維持することを正式には約束していません; 将来の Docker リリースで volumes の場所が変わった場合は、新しいパスを取り込むために `coast ssg import-host-volume` を再実行してください。

## パーミッション

いくつかのイメージは、データディレクトリの所有者が誤っていると起動を拒否します。よくあるのは Postgres（debian タグでは UID 999、alpine タグでは UID 70）、MySQL/MariaDB（UID 999）、MongoDB（UID 999）です。ホストディレクトリの所有者が root だと、Postgres は起動時に簡潔な "data directory has wrong ownership" を出して終了します。

修正は 1 コマンドです:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

これは `coast ssg run` の前に実行してください。ディレクトリがまだ存在しない場合、`coast ssg run` がデフォルト所有者で作成します（Linux では root、Docker Desktop 経由の macOS ではあなたのユーザー）。このデフォルトは通常 Postgres には誤りです。`coast ssg import-host-volume` 経由で来ていて、以前に `docker-compose up` が最初の起動時にその volume を chown 済みであれば、すでに問題ありません。

## `coast ssg doctor`

`coast ssg doctor` は、現在のプロジェクトの SSG に対して実行される読み取り専用チェックです（cwd の `Coastfile` の `[coast].name` または `--working-dir` から解決されます）。アクティブなビルド内の各 `(service, host-bind)` ペアごとに 1 件の finding を出力し、さらに secret 抽出の finding も出力します（[Secrets](SECRETS.md) を参照）。

既知の各イメージ（Postgres、MySQL、MariaDB、MongoDB）について、内蔵の UID/GID テーブルを参照し、各ホストパスに対する `stat(2)` と比較して、次を出力します:

- 所有者がそのイメージの期待値と一致する場合は `ok`。
- 一致しない場合は `warn`。メッセージには修正用の `chown` コマンドが含まれます。
- ディレクトリがまだ存在しない場合、または該当イメージが named volume のみを持つ場合（ホスト側から確認するものがない場合）は `info`。

イメージが既知イメージテーブルにないサービスは黙ってスキップされます。`ghcr.io/baosystems/postgis` のようなフォークはフラグされません -- doctor は誤った警告を出すくらいなら何も言わない方を選びます。

```bash
coast ssg doctor
```

Postgres ディレクトリの不一致がある場合の出力例:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor は何も変更しません。ホストファイルシステム上に置いたバイト列のパーミッションは、Coast が黙って変更する種類のものではありません。

## プラットフォームに関する注意

- **macOS Docker Desktop.** 生のホストパスは Settings -> Resources -> File Sharing に列挙されている必要があります。デフォルトには `/Users`、`/Volumes`、`/private`、`/tmp` が含まれます。`/var/coast-data` は macOS のデフォルト一覧には **含まれていません** -- 新しいパスには `$HOME/coast-data/...` を優先するか、`/var/coast-data` を File Sharing に追加してください。`/var/lib/docker/volumes/<name>/_data` 形式はホストパスでは *ありません* -- Docker が自分の VM 内で解決します -- そのため File Sharing への追加なしで動作します。
- **WSL2.** WSL ネイティブのパス（`~`、`/mnt/wsl/...`）を優先してください。`/mnt/c/...` も動作しますが、Windows ホストファイルシステムを橋渡しする 9P プロトコルのため低速です。
- **Linux.** 注意点はありません。

## ライフサイクル

- `coast ssg rm` -- SSG の外側の DinD コンテナを削除します。**volume の内容はそのまま**、host bind-mount の内容もそのまま、keystore もそのままです。同じ Docker volume を使う他のものも引き続き動作します。
- `coast ssg rm --with-data` -- **SSG のネストされた Docker デーモン内**に存在する volumes を削除します（先頭スラッシュのない `name:path` 形式）。host bind mounts と外部 Docker volumes はやはり変更されません -- Coast はそれらを所有していません。
- `coast ssg build` -- volumes には決して触れません。manifest と（`[secrets]` が宣言されている場合は）keystore 行だけを書き込みます。
- `coast ssg run` / `start` / `restart` -- host bind-mount ディレクトリが存在しなければ作成します（デフォルト所有者で -- [パーミッション](#パーミッション) を参照）。

## 関連項目

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- volume 構文を含む完全な TOML スキーマ
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- 非 SSG サービス向けの共有・分離・スナップショットシード volume 戦略
- [Building](BUILDING.md) -- manifest の生成元
- [Lifecycle](LIFECYCLE.md) -- volumes がいつ作成・停止・削除されるか
- [Secrets](SECRETS.md) -- ファイル注入された secrets は `~/.coast/ssg/runs/<project>/secrets/<basename>` に配置され、内部サービスへ read-only で bind-mount されます

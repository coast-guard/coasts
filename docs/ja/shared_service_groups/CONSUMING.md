# Shared Service Group の利用

consumer Coast は、consumer の `Coastfile` に 1 行のフラグを記述することで、プロジェクトの SSG 所有サービスをサービス単位で利用します。Coast の内部では、アプリコンテナは引き続き `postgres:5432` を認識します。デーモンのルーティング層が、そのトラフィックを安定した仮想ポート経由でプロジェクトの `<project>-ssg` 外側 DinD にリダイレクトします。

`from_group = true` が参照する SSG は、**常に consumer プロジェクト自身の SSG** です。プロジェクト間共有はありません。consumer の `[coast].name` が `cg` の場合、`from_group = true` は `cg-ssg` の `Coastfile.shared_service_groups` に対して解決されます。

## Syntax

`from_group = true` を指定した `[shared_services.<name>]` ブロックを追加します:

```toml
# Consumer Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = true       # overrides the SSG service's default
```

TOML キー（この例では `postgres`）は、プロジェクトの `Coastfile.shared_service_groups` で宣言されたサービス名と一致している必要があります。

## Forbidden Fields

`from_group = true` を指定した場合、以下のフィールドは parse 時に拒否されます:

- `image`
- `ports`
- `env`
- `volumes`

これらはすべて SSG 側に存在します。`from_group = true` と一緒にこれらのいずれかが現れた場合、`coast build` は次のように失敗します:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

## Allowed Overrides

consumer ごとに引き続き許可されるフィールドは 2 つあります:

- `inject` -- 接続文字列を公開する env-var またはファイルパス。異なる consumer プロジェクトは、同じ内容を異なる env-var 名で公開できます。
- `auto_create_db` -- `coast run` 時に、このサービス内に Coast がインスタンスごとのデータベースを作成するかどうか。SSG サービス自身の `auto_create_db` の値を上書きします。

## Conflict Detection

単一の Coastfile 内で同じ名前を持つ 2 つの `[shared_services.<name>]` ブロックは、parse 時に拒否されます。このルールはそのままです。

`from_group = true` を持つブロックが、プロジェクトの `Coastfile.shared_service_groups` で宣言されていない名前を参照している場合、`coast build` 時に失敗します:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

これは typo チェックです。別個の実行時の「drift」チェックはありません -- consumer と SSG 間の shape の不一致は build 時チェックで現れ、それ以降の実行時の不一致は、アプリの観点では自然に接続エラーとして表面化します。

## Auto-start

consumer に対する `coast run` は、プロジェクトの SSG がまだ実行中でない場合、自動的に起動します:

- SSG build は存在するが、コンテナは実行中でない -> デーモンは、プロジェクトの SSG mutex によって保護された状態で、`coast ssg start` 相当（またはコンテナが一度も作成されていない場合は `run`）を実行します。
- SSG build がまったく存在しない -> hard error:

  ```text
  Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
  ```

- SSG がすでに実行中 -> no-op、`coast run` は即座に続行します。

進捗イベント `SsgStarting` と `SsgStarted` が run ストリーム上で発火するため、[Coastguard](../concepts_and_terminology/COASTGUARD.md) はその起動を consumer プロジェクトに帰属できます。

## How Routing Works

consumer Coast の内部では、アプリコンテナは 3 つの要素によって `postgres:5432` をプロジェクトの SSG に解決します:

1. **Alias IP + `extra_hosts`** により、consumer の inner compose に `postgres -> <docker0 alias IP>` が追加されるため、`postgres` に対する DNS ルックアップが成功します。
2. **In-DinD socat** は `<alias>:5432` で待ち受け、`host.docker.internal:<virtual_port>` に転送します。仮想ポートは `(project, service, container_port)` に対して安定しています -- SSG が rebuild されても変化しません。
3. **Host socat** は `<virtual_port>` で待ち受け、`127.0.0.1:<dynamic>` に転送します。ここで `<dynamic>` は SSG コンテナが現在 publish しているポートです。host socat は SSG が rebuild されると更新されますが、consumer の in-DinD socat は変更不要です。

アプリコードと compose DNS は変わりません。プロジェクトを inline Postgres から SSG Postgres に移行するのは、小さな Coastfile の編集（`image`/`ports`/`env` を削除し、`from_group = true` を追加）と rebuild だけです。

完全な hop-by-hop の説明、ポートの概念、およびその理由については、[Routing](ROUTING.md) を参照してください。

## `auto_create_db`

SSG の Postgres または MySQL サービスで `auto_create_db = true` を指定すると、そのサービスを利用して実行されるすべての consumer Coast に対して、デーモンがそのサービス内に `{instance}_{project}` データベースを作成します。データベース名は inline の `[shared_services]` パターンが生成するものと一致するため、`inject` URL は `auto_create_db` が作成するデータベースと一致します。

作成は冪等です。データベースがすでに存在するインスタンスに対して `coast run` を再実行しても no-op です。基盤となる SQL は inline パスと同一であるため、どのパターンをプロジェクトが使用していても DDL 出力は完全に同一です。

consumer は SSG サービスの `auto_create_db` 値を上書きできます:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject` は接続文字列をアプリコンテナに公開します。形式は [Secrets](../coastfiles/SECRETS.md) と同じです: `"env:NAME"` は環境変数を作成し、`"file:/path"` は consumer の coast コンテナ内にファイルを書き込み、それを read-only で bind-mount して、stub 化されていないすべての inner compose サービスに渡します。

解決される文字列は、dynamic host port ではなく、canonical service name と canonical port を使用します。この不変性こそが要点です -- アプリコンテナは、SSG がたまたまどの dynamic port で publish しているかに関係なく、常に `postgres://coast:coast@postgres:5432/{db}` を認識します。

`env:NAME` と `file:/path` の両方が完全に実装されています。

この `inject` は **consumer 側** の secret パイプラインです: 値は canonical SSG metadata から `coast build` 時に計算され、consumer の coast DinD に注入されます。これは、**SSG 側** の `[secrets.*]` パイプライン（[Secrets](SECRETS.md) を参照）とは独立しています。後者は、SSG 自身のサービスが利用する値を抽出するものです。

## Remote Coasts

remote Coast（`coast assign --remote ...` で作成されるもの）は、reverse SSH tunnel を通じて local SSG に到達します。local デーモンは remote マシンから local 仮想ポートへ向けて `ssh -N -R <vport>:localhost:<vport>` を起動します。remote DinD の内部では、`extra_hosts: postgres: host-gateway` が `postgres` を remote の host-gateway IP に解決し、SSH トンネルがその先の同じ仮想ポート番号で local SSG を提供します。

トンネルの両側は、dynamic port ではなく **virtual** port を使用します。つまり、local で SSG を rebuild しても remote トンネルは無効になりません。

トンネルは `(project, remote_host, service, container_port)` ごとに coalesced されます -- 同じ remote 上の同じプロジェクトの複数の consumer インスタンスは 1 つの `ssh -R` プロセスを共有します。1 つの consumer を削除してもトンネルは teardown されず、最後の consumer が削除されたときだけ teardown されます。

実際上の影響:

- remote の shadow Coast が現在 SSG を利用している間は、`coast ssg stop` / `rm` は拒否されます。デーモンはブロックしている shadow を一覧表示するため、何が SSG を使用しているか分かります。
- `coast ssg stop --force`（または `rm --force`）は、最初に共有 `ssh -R` を teardown してから続行します。remote consumer が接続を失ってもよいと受け入れる場合に使用してください。

完全な remote-tunnel アーキテクチャについては [Routing](ROUTING.md) を、より広範な remote-machine セットアップについては [Remote Coasts](../remote_coasts/README.md) を参照してください。

## See Also

- [Routing](ROUTING.md) -- canonical / dynamic / virtual port の概念と完全なルーティングチェーン
- [Secrets](SECRETS.md) -- サービス側クレデンシャル用の SSG ネイティブな `[secrets.*]`（consumer 側の `inject` とは直交）
- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- `from_group = true` を含む完全な `[shared_services.*]` スキーマ
- [Lifecycle](LIFECYCLE.md) -- auto-start を含め、`coast run` が内部で何を行うか
- [Checkout](CHECKOUT.md) -- ad-hoc ツールのための host 側 canonical-port バインディング
- [Volumes](VOLUMES.md) -- マウントとパーミッション。SSG を rebuild した際に、新しい Postgres イメージがデータディレクトリの所有権を変更する場合に関連します

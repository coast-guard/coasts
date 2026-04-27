# `coast ssg` CLI リファレンス

すべての `coast ssg` サブコマンドは、既存の Unix ソケット経由で同じローカルデーモンと通信します。`coast shared-service-group` は `coast ssg` のエイリアスです。

ほとんどの動詞は、cwd の `Coastfile` の `[coast].name`（または `--working-dir <dir>`）からプロジェクトを解決します。`coast ssg ls` のみがプロジェクト横断です。

すべてのコマンドは、進行状況の出力を抑制し、最終的な要約またはエラーのみを表示するグローバルな `--silent` / `-s` フラグを受け付けます。

## Commands

### Build & inspect

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | `Coastfile.shared_service_groups` を解析し、任意の `[secrets.*]` を抽出し、イメージをプルし、成果物を `~/.coast/ssg/<project>/builds/<id>/` に書き込み、`latest_build_id` を更新し、古いビルドを削除します。[Building](BUILDING.md) を参照してください。 |
| `coast ssg ps` | このプロジェクトの SSG ビルドのサービス一覧を表示します（`manifest.json` とライブコンテナ状態を読み取ります）。[Lifecycle -> ps](LIFECYCLE.md#coast-ssg-ps) を参照してください。 |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | `~/.coast/ssg/<project>/builds/` 配下のすべてのビルド成果物を、タイムスタンプ、サービス数、および `(latest)` / `(pinned)` 注記付きで一覧表示します。 |
| `coast ssg ls` | デーモンが認識しているすべての SSG をプロジェクト横断で一覧表示します（project、status、build id、service count、created-at）。[Lifecycle -> ls](LIFECYCLE.md#coast-ssg-ls) を参照してください。 |

### Lifecycle

| Command | Summary |
|---------|---------|
| `coast ssg run` | `<project>-ssg` DinD を作成し、動的ホストポートを割り当て、シークレットを具現化し（宣言されている場合）、内部 compose スタックを起動します。[Lifecycle -> run](LIFECYCLE.md#coast-ssg-run) を参照してください。 |
| `coast ssg start` | 以前に作成され、停止している SSG を起動します。シークレットを再度具現化し、保持されている canonical-port checkout socat を再生成します。 |
| `coast ssg stop [--force]` | プロジェクトの SSG DinD を停止します。コンテナ、動的ポート、仮想ポート、および checkout 行を保持します。`--force` は最初にリモート SSH トンネルを破棄します。 |
| `coast ssg restart` | 停止してから起動します。コンテナと動的ポートは保持されます。 |
| `coast ssg rm [--with-data] [--force]` | プロジェクトの SSG DinD を削除します。`--with-data` は内部の名前付きボリュームを削除します。`--force` はリモート shadow consumer が存在しても続行します。ホストの bind-mount 内容には一切触れません。**Keystore にも一切触れません** -- それには `coast ssg secrets clear` を使用してください。 |

### Logs & exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | 外側の DinD または内部の 1 つのサービスのログをストリーミングします。`--follow` は Ctrl+C までストリーミングを続けます。 |
| `coast ssg exec [--service <name>] -- <cmd...>` | 外側の `<project>-ssg` コンテナまたは内部の 1 つのサービスに対して exec します。`--` 以降はそのまま渡されます。 |

### Routing & checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | サービスごとの canonical / dynamic / virtual ポートマッピングを表示し、該当する場合は `(checked out)` 注記を付けます。[Routing](ROUTING.md) を参照してください。 |
| `coast ssg checkout [--service <name> \| --all]` | ホスト側の socat を介して canonical ホストポートをバインドします（フォワーダーはプロジェクトの安定した virtual ポートを対象にします）。Coast インスタンス保持者は警告付きで追い出されます。不明なホストプロセスに対してはエラーになります。[Checkout](CHECKOUT.md) を参照してください。 |
| `coast ssg uncheckout [--service <name> \| --all]` | このプロジェクトの canonical-port socat を破棄します。追い出された Coast の自動復元は行いません。 |

### Diagnostics

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | 既知イメージサービスのホスト bind-mount 権限および、宣言済みだが未抽出の SSG シークレットについて、読み取り専用のチェックを行います。`ok` / `warn` / `info` の結果を出力します。[Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor) を参照してください。 |

### Build pinning

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | このプロジェクトの SSG を特定の `build_id` に固定します。`coast ssg run` と `coast build` は `latest_build_id` の代わりにこのピンを使用します。[Building -> Locking a project to a specific build](BUILDING.md#locking-a-project-to-a-specific-build) を参照してください。 |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | ピンを解除します。冪等です。 |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | このプロジェクトの現在のピンがあれば表示します。 |

### SSG-native secrets

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | `coast_image = "ssg:<project>"` 配下の暗号化された keystore エントリをすべて削除します。冪等です。SSG-native secrets を消去する唯一の動詞です -- `coast ssg rm` と `rm --with-data` は意図的にそれらを残します。[Secrets](SECRETS.md) を参照してください。 |

### Migration helper

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | ホスト Docker の名前付きボリュームのマウントポイントを解決し、等価な SSG bind-mount エントリを出力します（または適用します）。[Volumes -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume) を参照してください。 |

## Exit Codes

- `0` -- 成功。`doctor` のようなコマンドは警告を見つけた場合でも 0 を返します。これらはゲートではなく診断ツールです。
- Non-zero -- バリデーションエラー、Docker エラー、状態不整合、または remote-shadow ゲート拒否。

## See Also

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Routing](ROUTING.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Secrets](SECRETS.md)
- [Checkout](CHECKOUT.md)

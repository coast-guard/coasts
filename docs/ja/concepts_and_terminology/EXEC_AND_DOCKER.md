# Exec と Docker

`coast exec` は、Coast の DinD コンテナ内のシェルに入ります。作業ディレクトリは `/workspace` です。これは、Coastfile が存在する [bind-mounted project root](FILESYSTEM.md) です。これは、ホストマシンから Coast 内でコマンドを実行したり、ファイルを確認したり、サービスをデバッグしたりするための主要な方法です。

`coast docker` は、内部の Docker デーモンと直接やり取りするための補助コマンドです。

## `coast exec`

Coast インスタンス内でシェルを開きます:

```bash
coast exec dev-1
```

これにより、`/workspace` で `sh` セッションが開始されます。Coast コンテナは Alpine ベースなので、デフォルトシェルは `bash` ではなく `sh` です。

対話シェルに入らずに、特定のコマンドを実行することもできます:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
coast exec dev-1 --service web
coast exec dev-1 --service web -- php artisan test
```

インスタンス名の後に続くものはすべてコマンドとして渡されます。自分のコマンドに属するフラグと `coast exec` に属するフラグを分けるには、`--` を使用してください。

外側の Coast コンテナではなく、特定の compose サービスコンテナを対象にするには `--service <name>` を渡します。Coast のデフォルトのホスト UID:GID マッピングではなく、生のコンテナルートアクセスが必要な場合は `--root` を渡します。

### Working Directory

シェルは `/workspace` で開始されます。これは、ホストのプロジェクトルートがコンテナに bind mount されたものです。つまり、ソースコード、Coastfile、およびすべてのプロジェクトファイルがそこにあります:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

`/workspace` 配下のファイルに加えた変更は、ホストに即座に反映されます。これはコピーではなく bind mount です。

### Interactive vs Non-Interactive

stdin が TTY のとき（ターミナルで入力しているとき）、`coast exec` はデーモンを完全にバイパスし、完全な TTY パススルーのために `docker exec -it` を直接実行します。これは、色、カーソル移動、タブ補完、対話型プログラムがすべて期待どおりに動作することを意味します。

stdin がパイプまたはスクリプト経由の場合（CI、エージェントワークフロー、`coast exec dev-1 -- some-command | grep foo`）、リクエストはデーモンを通り、構造化された stdout、stderr、および終了コードを返します。

### File Permissions

exec はホストユーザーの UID:GID として実行されるため、Coast 内で作成されたファイルはホスト上で正しい所有権を持ちます。ホストとコンテナの間で権限の不一致は発生しません。

## `coast docker`

`coast exec` は DinD コンテナ自体の中でシェルを提供しますが、`coast docker` は **内部の** Docker デーモン、つまり compose サービスを管理しているものに対して Docker CLI コマンドを実行できます。

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps for the active Coast-managed stack
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

渡したすべてのコマンドには自動的に `docker` が前置されます。したがって、`coast docker dev-1 compose ps` は Coast コンテナ内で `docker compose ps` を実行し、内部デーモンと通信します。

### `coast exec` vs `coast docker`

違いは、何を対象にしているかです:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | DinD コンテナ内での `sh -c "ls /workspace"` | Coast コンテナ自体（プロジェクトファイル、インストール済みツール） |
| `coast exec dev-1 --service web` | 解決された内部サービスコンテナ内での `docker exec ... sh` | 特定の compose サービスコンテナ |
| `coast docker dev-1 ps` | DinD コンテナ内での `docker ps` | 内部 Docker デーモン（compose サービスコンテナ） |
| `coast docker dev-1 compose logs web` | DinD コンテナ内での `docker compose logs web` | 内部デーモン経由での特定 compose サービスのログ |

プロジェクトレベルの作業（テストの実行、依存関係のインストール、ファイルの確認）には `coast exec` を使用します。内部 Docker デーモンが何をしているか（コンテナの状態、イメージ、ネットワーク、compose 操作）を確認する必要がある場合は `coast docker` を使用します。

## Coastguard Exec Tab

Coastguard の Web UI は、WebSocket 経由で接続された永続的な対話型ターミナルを提供します。

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Coast インスタンス内の /workspace でのシェルセッションを表示している Coastguard Exec タブ。*

このターミナルは xterm.js によって提供され、以下を備えています:

- **永続セッション** — ターミナルセッションは、ページ移動やブラウザのリフレッシュ後も維持されます。再接続するとスクロールバックバッファが再生されるため、中断したところから再開できます。
- **複数タブ** — 複数のシェルを同時に開けます。各タブは独立したセッションです。
- **[Agent shell](AGENT_SHELLS.md) タブ** — AI コーディングエージェント用の専用エージェントシェルを起動でき、アクティブ／非アクティブ状態の追跡があります。
- **フルスクリーンモード** — ターミナルを画面いっぱいに拡大します（終了は Escape）。

インスタンスレベルの exec タブに加えて、Coastguard は他のレベルでもターミナルアクセスを提供します:

- **Service exec** — Services タブから個別のサービスに入ると、その特定の内部コンテナ内のシェルを取得できます（これは二重の `docker exec` を行います。最初に DinD コンテナに入り、次にサービスコンテナに入ります）。
- **[Shared service](SHARED_SERVICES.md) exec** — ホストレベルの共有サービスコンテナ内のシェルを取得できます。
- **Host terminal** — Coast に入ることなく、プロジェクトルートでホストマシン上のシェルを使えます。

## When to Use Which

- **`coast exec`** — DinD コンテナ内でプロジェクトレベルのコマンドを実行する、または `--service` を渡して特定の compose サービスコンテナ内でシェルを開くかコマンドを実行します。
- **`coast docker`** — 内部 Docker デーモンを確認または管理します（コンテナの状態、イメージ、ネットワーク、compose 操作）。
- **Coastguard Exec tab** — 永続セッション、複数タブ、エージェントシェル対応を備えた対話型デバッグ向け。UI の他の部分を移動しながら複数のターミナルを開いたままにしたい場合に最適です。
- **`coast logs`** — サービス出力を読むには、`coast docker compose logs` ではなく `coast logs` を使用してください。[Logs](LOGS.md) を参照してください。
- **`coast ps`** — サービス状態を確認するには、`coast docker compose ps` ではなく `coast ps` を使用してください。[Runtimes and Services](RUNTIMES_AND_SERVICES.md) を参照してください。

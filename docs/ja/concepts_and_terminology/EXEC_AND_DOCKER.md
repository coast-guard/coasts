# Exec & Docker

`coast exec` は、Coast の DinD コンテナ内のシェルへ入ります。作業ディレクトリは `/workspace` — Coastfile が置かれている [bind-mounted なプロジェクトルート](FILESYSTEM.md) です。これはホストマシンから Coast 内でコマンドを実行したり、ファイルを調べたり、サービスをデバッグしたりするための主要な方法です。

`coast docker` は、内側の Docker デーモンと直接やり取りするための補助コマンドです。

## `coast exec`

Coast インスタンス内でシェルを開きます:

```bash
coast exec dev-1
```

これは `/workspace` で `sh` セッションを開始します。Coast コンテナは Alpine ベースのため、デフォルトのシェルは `bash` ではなく `sh` です。

対話シェルに入らずに、特定のコマンドだけを実行することもできます:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
```

インスタンス名の後ろに続くものはすべてコマンドとして渡されます。`--` を使って、実行したいコマンド側のフラグと `coast exec` 側のフラグを分離してください。

### Working Directory

シェルは `/workspace` から開始します。これはホストのプロジェクトルートがコンテナに bind-mount されたものです。つまり、ソースコード、Coastfile、そしてすべてのプロジェクトファイルがそこにあります:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

`/workspace` 配下のファイルに加えた変更は、ホスト側に即座に反映されます — コピーではなく bind mount です。

### Interactive vs Non-Interactive

stdin が TTY（ターミナルで入力している状態）の場合、`coast exec` はデーモンを完全にバイパスし、フル TTY パススルーのために `docker exec -it` を直接実行します。これにより、色表示、カーソル移動、タブ補完、対話的プログラムが期待どおり動作します。

stdin がパイプやスクリプト（CI、エージェントワークフロー、`coast exec dev-1 -- some-command | grep foo`）の場合、リクエストはデーモン経由で処理され、構造化された stdout、stderr、および終了コードが返されます。

### File Permissions

exec はホストユーザーの UID:GID として実行されるため、Coast 内で作成したファイルはホスト上でも正しい所有権になります。ホストとコンテナ間で権限の不一致が起きません。

## `coast docker`

`coast exec` が DinD コンテナ自体のシェルを提供するのに対し、`coast docker` は **内側の** Docker デーモン — compose サービスを管理しているデーモン — に対して Docker CLI コマンドを実行できます。

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps (inner services)
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

渡したすべてのコマンドには自動的に `docker` が前置されます。したがって、`coast docker dev-1 compose ps` は Coast コンテナ内で `docker compose ps` を実行し、内側のデーモンと通信します。

### `coast exec` vs `coast docker`

違いは「何を対象にしているか」です:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | DinD コンテナ内で `sh -c "ls /workspace"` | Coast コンテナ自体（プロジェクトファイル、インストール済みツール） |
| `coast docker dev-1 ps` | DinD コンテナ内で `docker ps` | 内側の Docker デーモン（compose サービスのコンテナ） |
| `coast docker dev-1 compose logs web` | DinD コンテナ内で `docker compose logs web` | 内側のデーモン経由で特定の compose サービスのログ |

プロジェクトレベルの作業 — テスト実行、依存関係のインストール、ファイルの調査 — には `coast exec` を使ってください。内側の Docker デーモンが何をしているか — コンテナ状態、イメージ、ネットワーク、compose 操作 — を確認したい場合は `coast docker` を使ってください。

## Coastguard Exec Tab

Coastguard の Web UI は、WebSocket 経由で接続される永続的な対話ターミナルを提供します。

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Coastguard の Exec タブ。Coast インスタンス内の /workspace でのシェルセッションを表示。*

このターミナルは xterm.js によって提供され、以下を備えています:

- **永続セッション** — ターミナルセッションはページ移動やブラウザの更新をまたいで維持されます。再接続時にはスクロールバックバッファが再生され、中断したところから続けられます。
- **複数タブ** — 複数のシェルを同時に開けます。各タブは独立したセッションです。
- **[Agent shell](AGENT_SHELLS.md) タブ** — AI コーディングエージェント向けの専用エージェントシェルを起動し、アクティブ/非アクティブ状態の追跡ができます。
- **フルスクリーンモード** — ターミナルを画面全体に拡大します（Escape で終了）。

インスタンスレベルの exec タブに加えて、Coastguard は他のレベルでもターミナルアクセスを提供します:

- **Service exec** — Services タブから個別のサービスをクリックし、その特定の内側コンテナ内のシェルに入ります（`docker exec` を二重に実行 — まず DinD コンテナへ、次にサービスコンテナへ）。
- **[Shared service](SHARED_SERVICES.md) exec** — ホストレベルの共有サービスコンテナ内のシェルに入ります。
- **Host terminal** — Coast に入らずに、プロジェクトルートでホストマシン上のシェルを開きます。

## When to Use Which

- **`coast exec`** — DinD コンテナ内で、プロジェクトレベルのコマンド（npm install、go test、ファイル調査、デバッグ）を実行します。
- **`coast docker`** — 内側の Docker デーモンを調査または管理します（コンテナ状態、イメージ、ネットワーク、compose 操作）。
- **Coastguard Exec tab** — 永続セッション、複数タブ、エージェントシェル対応による対話的デバッグ。UI の他の部分を操作しながら複数のターミナルを開いたままにしたい場合に最適です。
- **`coast logs`** — サービス出力を読むには、`coast docker compose logs` の代わりに `coast logs` を使用します。[Logs](LOGS.md) を参照してください。
- **`coast ps`** — サービス状態を確認するには、`coast docker compose ps` の代わりに `coast ps` を使用します。[Runtimes and Services](RUNTIMES_AND_SERVICES.md) を参照してください。

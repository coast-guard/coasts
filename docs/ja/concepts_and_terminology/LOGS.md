# ログ

Coast 内のサービスはネストされたコンテナ内で動作します。つまり、あなたの compose サービスは DinD コンテナ内の内側の Docker デーモンによって管理されています。これは、ホストレベルのロギングツールからはそれらが見えないことを意味します。ホスト上で Docker ログを読む logging MCP をワークフローに含めている場合、見えるのは外側の DinD コンテナだけで、その内側で動作している Web サーバー、データベース、ワーカーは見えません。

解決策は `coast logs` です。Coast インスタンスからサービス出力を読む必要があるエージェントやツールは、ホストレベルの Docker ログアクセスではなく Coast CLI を使用する必要があります。

## MCP のトレードオフ

logging MCP（ホストから Docker コンテナログを取得するツール — [MCP Servers](MCP_SERVERS.md) を参照）を備えた AI エージェントを使用している場合、その MCP は Coast 内で動作しているサービスには機能しません。ホストの Docker デーモンからは Coast インスタンスごとに 1 つのコンテナ（DinD コンテナ）しか見えず、そのログは内側の Docker デーモンの起動出力にすぎません。

実際のサービスログを取得するには、エージェントに次を使用するよう指示してください:

```bash
coast logs <instance> --service <service> --tail <lines>
```

たとえば、バックエンドサービスが失敗している理由をエージェントが調査する必要がある場合:

```bash
coast logs dev-1 --service backend --tail 100
```

これは `docker compose logs` と同等ですが、Coast デーモン経由で内側の DinD コンテナにルーティングされます。logging MCP を参照するエージェントルールや system prompt がある場合、Coast 内で作業するときにこの挙動を上書きする指示を追加する必要があります。

## `coast logs`

CLI は Coast インスタンスからログを読むためのいくつかの方法を提供します:

```bash
coast logs dev-1                           # last 200 lines, all services
coast logs dev-1 --service web             # last 200 lines, web only
coast logs dev-1 --tail 50                 # last 50 lines, then follow
coast logs dev-1 --tail                    # all lines, then follow
coast logs dev-1 --service backend -f      # follow mode (stream new entries)
coast logs dev-1 --service web --tail 100  # last 100 lines + follow
```

`--tail` または `-f` がない場合、コマンドは最後の 200 行を返して終了します。`--tail` を付けると、指定した行数をストリームし、その後も新しい出力をリアルタイムで追従し続けます。`-f` / `--follow` は単独で follow モードを有効にします。

出力は compose のログ形式を使用し、各行にサービスのプレフィックスが付きます:

```text
web       | 2026/02/28 01:49:34 Listening on :3000
backend   | 2026/02/28 01:49:34 [INFO] Server started on :8080
backend   | 2026/02/28 01:49:34 [ProcessCreditsJob] starting at 2026-02-28T01:49:34Z
redis     | 1:M 28 Feb 2026 01:49:30.123 * Ready to accept connections
```

レガシーな位置引数の構文（`coast logs dev-1 web`）でもサービスでフィルタできますが、`--service` フラグの使用が推奨されます。

## Coastguard の Logs タブ

Coastguard の Web UI は、WebSocket 経由のリアルタイムストリーミングを備えた、よりリッチなログ閲覧体験を提供します。

![Logs tab in Coastguard](../../assets/coastguard-logs.png)
*サービスフィルタと検索を備え、バックエンドサービス出力をストリーミングしている Coastguard の Logs タブ。*

Logs タブが提供する機能:

- **リアルタイムストリーミング** — ログは生成されると同時に WebSocket 接続を通じて到着し、接続状態を示すステータスインジケータがあります。
- **サービスフィルタ** — ログストリームのサービスプレフィックスから生成されるドロップダウン。単一サービスを選択してその出力に集中できます。
- **検索** — 表示されている行をテキストまたは正規表現でフィルタ（アスタリスクボタンで正規表現モードを切り替え）。一致した用語はハイライトされます。
- **行数** — フィルタ後の行数と総行数を表示（例: "200 / 971 lines"）。
- **Clear** — 内側コンテナのログファイルを切り詰め、ビューアをリセットします。
- **Fullscreen** — ログビューアを画面いっぱいに拡大します。

ログ行は ANSI カラーに対応して描画され、ログレベルの強調表示（ERROR は赤、WARN は琥珀色、INFO は青、DEBUG は灰色）、タイムスタンプの減光、サービス間の視覚的な区別のための色付きサービスバッジが提供されます。

ホストデーモン上で動作する共有サービスは、Shared Services タブからアクセスできる専用のログビューアを持ちます。詳細は [Shared Services](SHARED_SERVICES.md) を参照してください。

## 仕組み

`coast logs` を実行すると、デーモンは `docker exec` を介して DinD コンテナ内で `docker compose logs` を実行し、出力を端末（または Coastguard UI へ WebSocket 経由）にストリームで返します。

```text
coast logs dev-1 --service web --tail 50
  │
  ├── CLI sends LogsRequest to daemon (Unix socket)
  │
  ├── Daemon resolves instance → container ID
  │
  ├── Daemon exec's into DinD container:
  │     docker compose logs --tail 50 --follow web
  │
  └── Output streams back chunk by chunk
        └── CLI prints to stdout / Coastguard renders in UI
```

[bare services](BARE_SERVICES.md) の場合、デーモンは `docker compose logs` を呼び出す代わりに `/var/log/coast-services/` のログファイルを tail します。出力形式は同じ（`service  | line`）なので、どちらの場合もサービスフィルタリングは同様に機能します。

## 関連コマンド

- `coast ps <instance>` — 実行中のサービスとそのステータスを確認します。[Runtimes and Services](RUNTIMES_AND_SERVICES.md) を参照してください。
- [`coast exec <instance>`](EXEC_AND_DOCKER.md) — 手動デバッグのために Coast コンテナ内でシェルを開きます。

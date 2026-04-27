# SSG ホスト側チェックアウト

Consumer Coast は、デーモンのルーティング層（in-DinD socat -> host socat -> dynamic port）を通じて SSG サービスに到達します。これはアプリコンテナには非常に有効です。しかし、`localhost:5432` に対して、まるでそのサービスがその場で動作しているかのように接続したいホスト側の呼び出し元 -- MCP、アドホックな `psql` セッション、エディタのデータベースインスペクタ -- には役立ちません。

`coast ssg checkout` はその問題を解決します。これは、正規のホストポート（Postgres なら 5432、Redis なら 6379、...）をバインドし、プロジェクトの安定した仮想ポートへ転送するホストレベルの socat を起動します。そこから先は、ホスト上に既存の virtual-port socat がトラフィックを SSG の現在公開されている dynamic port に運びます。

この仕組み全体はプロジェクトごとです。`coast ssg checkout --service postgres` は、cwd の `Coastfile` を所有するプロジェクトに解決されます。もしこのマシン上に 2 つのプロジェクトがある場合でも、一度に正規ポート 5432 を保持できるのは 1 つだけです。

## Usage

```bash
coast ssg checkout --service postgres     # bind one service
coast ssg checkout --all                  # bind every SSG service
coast ssg uncheckout --service postgres   # tear down one
coast ssg uncheckout --all                # tear down every active checkout
```

チェックアウトが成功した後、`coast ssg ports` は各バインド済みサービスに `(checked out)` を注記します:

```text
  SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
  postgres             5432            54201           42000      (checked out)
  redis                6379            54202           42001
```

Consumer Coast は、ホスト側のチェックアウト状態に関係なく、常に in-DinD socat -> virtual port チェーン経由で SSG サービスに到達します。チェックアウトは純粋にホスト側の利便性のためのものです。

## Two-Hop Forwarder

チェックアウト用 socat は、SSG の dynamic host port を**直接**指しません。代わりに、プロジェクトの安定した virtual port を指します:

```text
host process            -> 127.0.0.1:5432           (checkout socat, listens here)
                        -> 127.0.0.1:42000          (project's virtual port)
                        -> 127.0.0.1:54201          (SSG's current dynamic port)
                        -> <project>-ssg postgres   (inner service)
```

この 2 ホップのチェーンにより、dynamic port が変化しても、SSG の再ビルドをまたいでチェックアウト用 socat は動作し続けます。更新されるのはホストの virtual-port socat だけであり、canonical-port socat はそれを認識しません。ホストの socat 層がどのように維持されるかについては [Routing](ROUTING.md) を参照してください。

## Displacement of Coast-Instance Holders

SSG に正規ポートのチェックアウトを要求したとき、そのポートはすでに保持されている可能性があります。セマンティクスは、それを誰が保持しているかによって異なります:

- **明示的にチェックアウトされた Coast インスタンス。** その日の早い時点で何らかの Coast に対して `coast checkout <instance>` を実行し、`localhost:5432` をその Coast の内部 Postgres にバインドしていた場合です。SSG のチェックアウトはそれを**置き換えます**: デーモンは既存の socat を kill し、その Coast の `port_allocations.socat_pid` をクリアし、代わりに SSG の socat をバインドします。CLI は明確な警告を表示します:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432 -> virtual 42000.
  ```

  置き換えられた Coast は、後で `coast ssg uncheckout` しても**自動的には再バインドされません**。その dynamic port は引き続き動作しますが、正規ポートは `coast checkout my-app/dev-2` を再度実行するまで未バインドのままです。

- **別プロジェクトの SSG チェックアウト。** もし `filemap-ssg` がすでに 5432 をチェックアウトしていて、そこに `cg-ssg` の 5432 をチェックアウトしようとした場合、デーモンは保持者を明示したわかりやすいメッセージとともに拒否します。先に `filemap-ssg` の 5432 を uncheckout してください。

- **`socat_pid` が死んでいる以前の SSG checkout row。** デーモンのクラッシュや stop/start サイクルによって残った古いメタデータです。新しいチェックアウトはその row を黙って再取得します。

- **その他すべて**（手動で起動したホスト Postgres、別のデーモン、ポート 8080 上の `nginx`）。この場合 `coast ssg checkout` はエラーになります:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  `--force` フラグはありません。未知のプロセスを黙って kill するのは危険すぎると判断されました。

## Stop / Start Behavior

`coast ssg stop` は、生きている canonical-port socat プロセスを kill しますが、**チェックアウト行自体は state DB に保持されます**。

`coast ssg run` / `start` / `restart` は保持された行を反復処理し、各行ごとに新しい canonical-port socat を再生成します。canonical port（5432）は同一のままです。`run` サイクル間で変化するのは dynamic port だけであり、チェックアウト用 socat は安定した **virtual** port を対象にしているため、再バインドは機械的です。

再ビルドされた SSG からサービスが消えた場合、その checkout row は run レスポンス内の警告とともに削除されます:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm` は、そのプロジェクトのすべての `ssg_port_checkouts` row を消去します。`rm` は設計上破壊的です -- 明示的にクリーンな初期状態を要求したためです。

## Daemon Restart Recovery

予期しないデーモン再起動（クラッシュ、`coastd restart`、再起動）の後、`restore_running_state` は `ssg_port_checkouts` テーブルを参照し、現在の dynamic / virtual port 割り当てに対してすべての row を再生成します。`localhost:5432` はデーモンの変動をまたいでもバインドされたままです。

## When to Check Out

- プロジェクトの SSG Postgres に GUI データベースクライアントを向けたいとき。
- 最初に dynamic port を調べなくても `psql "postgres://coast:coast@localhost:5432/mydb"` を動作させたいとき。
- ホスト上の MCP が安定した正規エンドポイントを必要とするとき。
- Coastguard が SSG の HTTP admin port をプロキシしたいとき。

**チェックアウトしない**ほうがよい場合:

- consumer Coast の内部からの接続性のため -- それはすでに in-DinD socat から virtual port を通じて機能しています。
- `coast ssg ports` の出力を使い、その dynamic port をツールに入力することに問題がないとき。

## See Also

- [Routing](ROUTING.md) -- canonical / dynamic / virtual port の概念と完全なホスト側フォワーダチェーン
- [Lifecycle](LIFECYCLE.md) -- stop / start / rm の詳細
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- この考え方の Coast インスタンス版
- [Ports](../concepts_and_terminology/PORTS.md) -- システム全体にわたる canonical と dynamic port の配線

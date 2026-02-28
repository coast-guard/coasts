# Coast CLI

Coast CLI（`coast`）は、Coast を操作するための主要なコマンドライン・インターフェースです。意図的に薄く作られており、コマンドを解析して [`coastd`](DAEMON.md) にリクエストを送信し、構造化された出力をターミナルに表示します。

## 何に使うのか

典型的なワークフローはすべて CLI から駆動されます:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Coasts
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

CLI には、人間とエージェントに役立つドキュメント用コマンドも含まれています:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## なぜデーモンと別に存在するのか

CLI とデーモンを分離することで、いくつかの重要な利点があります:

- デーモンは状態と長時間稼働するプロセスを保持します。
- CLI は高速で、合成可能で、スクリプト化しやすいままです。
- ターミナルの状態を生かしたままにせず、ワンオフのコマンドを実行できます。
- エージェント用ツールは、予測可能で自動化に適した方法で CLI コマンドを呼び出せます。

## CLI と Coastguard の比較

その時々に合うインターフェースを使ってください:

- CLI は運用の全範囲をカバーするよう設計されています: Coastguard でできることは、CLI からもできるべきです。
- CLI は自動化インターフェースとして扱ってください — スクリプト、エージェントのワークフロー、CI ジョブ、カスタム開発者ツール。
- [Coastguard](COASTGUARD.md) は人間向けインターフェースとして扱ってください — 視覚的な確認、対話的なデバッグ、運用の可視性。

どちらも同じデーモンと通信するため、同じ基盤となるプロジェクト状態を操作します。

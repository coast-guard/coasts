# Coast CLI

Coast CLI（`coast`）は、Coast を操作するための主要なコマンドラインインターフェースです。意図的に薄い設計になっており、コマンドを解析し、[`coastd`](DAEMON.md) にリクエストを送信し、構造化された出力を端末に表示します。

## 何に使うのか

一般的なワークフローはすべて CLI から実行されます。

```bash
coast build                                    # see Builds
coast run dev-1                                # see Run
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

CLI には、人間とエージェントの両方に役立つドキュメント用コマンドも含まれています。

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## なぜデーモンから分離されているのか

CLI をデーモンから分離することで、いくつかの重要な利点があります。

- デーモンは状態と長寿命のプロセスを保持します。
- CLI は高速で、組み合わせやすく、スクリプト化しやすいままです。
- ターミナルの状態を維持したままにしなくても、単発のコマンドを実行できます。
- エージェント用ツールは、予測可能で自動化に適した方法で CLI コマンドを呼び出せます。

## CLI と Coastguard

そのときに適したインターフェースを使ってください。

- CLI は運用全体をカバーするように設計されています。Coastguard でできることは、CLI からもできるはずです。
- CLI は自動化インターフェースとして扱ってください — スクリプト、エージェントのワークフロー、CI ジョブ、カスタム開発者ツール向けです。
- [Coastguard](COASTGUARD.md) は人間向けインターフェースとして扱ってください — 視覚的な確認、対話的なデバッグ、運用の可視性向けです。

どちらも同じデーモンと通信するため、同じ基盤となるプロジェクト状態に対して動作します。

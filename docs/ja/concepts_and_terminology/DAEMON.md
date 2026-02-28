# Coast Daemon

Coast デーモン（`coastd`）は、実際のオーケストレーション作業を行う長時間稼働のローカルプロセスです。[CLI](CLI.md) と [Coastguard](COASTGUARD.md) はクライアントであり、`coastd` はそれらの背後にあるコントロールプレーンです。

## Architecture at a Glance

```text
coast CLI (automation) -----+
                            +--> coastd daemon
Coastguard UI (human) ------+         |
                                      +--> Coasts
                                      +--> Ports
                                      +--> State
```

CLI はローカルの Unix ソケット経由で `coastd` にリクエストを送信し、Coastguard は WebSocket 経由で接続します。デーモンは実行時状態に変更を適用します。

## What It Does

`coastd` は、永続的な状態とバックグラウンドでの調整を必要とする操作を処理します:

- Coast インスタンス、ビルド、共有サービスを追跡します。
- Coast ランタイムの作成、起動、停止、削除を行います。
- assign/unassign/checkout 操作を適用します。
- 正規および動的な [port forwarding](PORTS.md) を管理します。
- [logs](LOGS.md)、ステータス、ランタイムイベントを CLI および UI クライアントへストリーミングします。

要するに: `coast run`、`coast assign`、`coast checkout`、または `coast ls` を実行する場合、作業を行うコンポーネントはデーモンです。

## How It Runs

デーモンは一般的に次の 2 つの方法で実行できます:

```bash
# Register daemon auto-start at login (recommended)
coast daemon install

# Manual start mode
coast daemon start
```

daemon install を省略した場合、Coast コマンドを使用する前に、各セッションで自分で起動する必要があります。

## Reporting Bugs

問題に遭遇した場合は、バグレポートを提出する際に `coastd` デーモンログを含めてください。ログには、ほとんどの問題を診断するのに必要なコンテキストが含まれています:

```bash
coast daemon logs
```

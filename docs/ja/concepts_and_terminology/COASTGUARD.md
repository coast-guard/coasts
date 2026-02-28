# Coastguard

Coastguard は Coast のローカル Web UI（Coast の Docker Desktop 風インターフェースを想定）で、ポート `31415` で動作します。CLI から起動します:

```bash
coast ui
```

![Coastguard project overview](../../assets/coastguard-overview.png)
*実行中の Coast インスタンス、それらのブランチ/ワークツリー、およびチェックアウト状態を表示するプロジェクトダッシュボード。*

![Coastguard port mappings](../../assets/coastguard-ports.png)
*特定の Coast インスタンスの ports ページ。各サービスの標準（canonical）および動的なポートマッピングを表示します。*

## What Coastguard Is Good For

Coastguard は、プロジェクトのための視覚的な制御および可観測性のための画面を提供します:

- プロジェクト、インスタンス、ステータス、ブランチ、チェックアウト状態を確認。
- [ポートマッピング](PORTS.md) を確認し、サービスに直接ジャンプ。
- [ログ](LOGS.md)、ランタイム統計を表示し、データを検査。
- [ビルド](BUILDS.md)、イメージ成果物、[ボリューム](VOLUMES.md)、[シークレット](SECRETS.md) のメタデータを参照。
- 作業しながらアプリ内でドキュメントを閲覧。

## Relationship to CLI and Daemon

Coastguard は CLI を置き換えるものではありません。人間向けのインターフェースとしてそれを補完します。

- [`coast` CLI](CLI.md) は、スクリプト、エージェントのワークフロー、ツール連携のための自動化インターフェースです。
- Coastguard は、視覚的な確認、対話的なデバッグ、日常的な運用可視性のための人間向けインターフェースです。
- どちらも [`coastd`](DAEMON.md) のクライアントであるため、同期された状態を保ちます。

# リモートコースト

> **ベータ版。** リモートコーストは完全に機能しますが、CLI フラグ、Coastfile スキーマ、および coast-service API は将来のリリースで変更される可能性があります。バグや不具合を見つけた場合は、プルリクエストを送るか issue を作成してください。

リモートコーストは、開発体験をローカルコーストと同一のまま維持しつつ、リモートマシン上であなたのサービスを実行します。`coast run`、`coast assign`、`coast exec`、`coast ps`、`coast logs`、およびその他すべてのコマンドは同じように動作します。デーモンはインスタンスがリモートであることを検出し、SSH トンネルを介して透過的に操作をルーティングします。

## なぜリモートなのか

ローカルコーストは、すべてをあなたのノートパソコン上で実行します。各コーストインスタンスは、compose スタック全体を含む完全な Docker-in-Docker コンテナを実行します: Web サーバー、API、ワーカー、データベース、キャッシュ、メールサーバー。これは、あなたのノートパソコンの RAM やディスク容量が不足するまではうまく機能します。

複数のサービスを持つフルスタックプロジェクトでは、コーストごとにかなりの RAM を消費することがあります。いくつかのコーストを並列に実行すると、ノートパソコンの限界に達します。

```text
  coast-1         coast-2         coast-3         coast-4
  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ worker   │   │ worker   │   │ worker   │   │ worker   │
  │ api      │   │ api      │   │ api      │   │ api      │
  │ admin    │   │ admin    │   │ admin    │   │ admin    │
  │ web      │   │ web      │   │ web      │   │ web      │
  │ mailhog  │   │ mailhog  │   │ mailhog  │   │ mailhog  │
  │          │   │          │   │          │   │          │
  │ 12 GB    │   │ 12 GB    │   │ 12 GB    │   │ 12 GB    │
  └──────────┘   └──────────┘   └──────────┘   └──────────┘

  Total: 48 GB RAM on your laptop
```

リモートコーストを使うと、一部のコーストをリモートマシンに移すことで水平方向にスケールできます。DinD コンテナ、compose サービス、およびイメージビルドはリモートで実行される一方、エディタとエージェントはローカルに残ります。Postgres や Redis のような共有サービスもローカルに残り、SSH リバーストンネルを通じてローカルおよびリモートインスタンス間でデータベースの同期が維持されます。

```text
  Your Machine                         Remote Server
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agents    │             │  coast-1 (all services) │
  │                     │  SSH        │  coast-2 (all services) │
  │  shared services    │──tunnels──▶ │  coast-3 (all services) │
  │  (postgres, redis)  │             │  coast-4 (all services) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: lightweight                  Server: 64 GB RAM, 16 CPU
```

localhost ランタイムを水平方向にスケールしましょう。

## クイックスタート

```bash
# 1. Register a remote machine
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote test my-vm

# 2. Build on the remote (uses remote's native architecture)
coast build --type remote

# 3. Run a remote coast
coast run dev-1 --type remote

# 4. Everything works as usual
coast ps dev-1
coast exec dev-1 -- bash
coast assign dev-1 --worktree feature/x
coast checkout dev-1
```

ホストの準備や coast-service のデプロイを含む完全なセットアップ手順については、[Setup](SETUP.md) を参照してください。

## リファレンス

| Page | What it covers |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | 2 コンテナ分割（shell coast + remote coast）、SSH トンネルレイヤー、ポートフォワーディングチェーン、およびデーモンがどのようにリクエストをルーティングするか |
| [Setup](SETUP.md) | ホスト要件、coast-service のデプロイ、リモートの登録、およびエンドツーエンドのクイックスタート |
| [File Sync](FILE_SYNC.md) | 一括転送のための rsync、継続同期のための mutagen、run/assign/stop をまたぐライフサイクル、除外設定、および競合状態の処理 |
| [Builds](BUILDS.md) | ネイティブアーキテクチャ向けにリモート上でビルドすること、成果物の転送、`latest-remote` シンボリックリンク、アーキテクチャの再利用、および自動プルーニング |
| [CLI and Configuration](CLI.md) | `coast remote` コマンド、`Coastfile.remote` 設定、ディスク管理、および `coast remote prune` |

## 関連項目

- [Remotes](../concepts_and_terminology/REMOTES.md) -- 用語集における概念の概要
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- ローカル共有サービスがどのようにリモートコーストへリバーストンネルされるか
- [Ports](../concepts_and_terminology/PORTS.md) -- SSH トンネルレイヤーが canonical/dynamic ポートモデルにどのように適合するか

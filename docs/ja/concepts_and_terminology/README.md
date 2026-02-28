# 概念と用語

このセクションでは、Coasts 全体で使用される中核となる概念と語彙を扱います。Coasts が初めての場合は、設定や高度な使い方に入る前にここから始めてください。

- [Coasts](COASTS.md) — プロジェクトの自己完結型ランタイム。それぞれが独自のポート、ボリューム、worktree の割り当てを持ちます。
- [Filesystem](FILESYSTEM.md) — ホストと Coast の間で共有されるマウント、ホスト側エージェント、および worktree の切り替え。
- [Coast Daemon](DAEMON.md) — ライフサイクル操作を実行するローカルの `coastd` コントロールプレーン。
- [Coast CLI](CLI.md) — コマンド、スクリプト、エージェントワークフローのためのターミナルインターフェース。
- [Coastguard](COASTGUARD.md) — 観測性と制御のために `coast ui` で起動される Web UI。
- [Ports](PORTS.md) — 標準（canonical）ポートと動的ポートの違い、および checkout がそれらの間をどのようにスワップするか。
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — プライマリサービスへのクイックリンク、Cookie 分離のためのサブドメインルーティング、URL テンプレート。
- [Assign and Unassign](ASSIGN.md) — Coast を worktree 間で切り替える方法と、利用可能な assign 戦略。
- [Checkout](CHECKOUT.md) — 標準ポートを Coast インスタンスにマッピングすること、およびそれが必要になる場面。
- [Lookup](LOOKUP.md) — エージェントの現在の worktree に一致する Coast インスタンスを見つける。
- [Volume Topology](VOLUMES.md) — 共有サービス、共有ボリューム、分離ボリューム、スナップショット。
- [Shared Services](SHARED_SERVICES.md) — ホスト管理のインフラサービスとボリュームの曖昧性解消。
- [Secrets and Extractors](SECRETS.md) — ホストのシークレットを抽出し、Coast コンテナへ注入する。
- [Builds](BUILDS.md) — coast build の構造、成果物の保存場所、自動プルーニング、型付きビルド。
- [Coastfile Types](COASTFILE_TYPES.md) — extends、unset、omit、autostart を備えた合成可能な Coastfile バリアント。
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — DinD ランタイム、Docker-in-Docker アーキテクチャ、そしてサービスが Coast 内でどのように動作するか。
- [Bare Services](BARE_SERVICES.md) — Coast 内でコンテナ化されていないプロセスを実行すること、および代わりにコンテナ化すべき理由。
- [Logs](LOGS.md) — Coast 内からサービスログを読む方法、MCP のトレードオフ、Coastguard のログビューア。
- [Exec & Docker](EXEC_AND_DOCKER.md) — Coast 内でコマンドを実行することと、内側の Docker デーモンと通信すること。
- [Agent Shells](AGENT_SHELLS.md) — コンテナ化されたエージェント TUI、OAuth のトレードオフ、そしておそらくエージェントはホスト上で実行すべき理由。
- [MCP Servers](MCP_SERVERS.md) — コンテナ化されたエージェント向けに Coast 内で MCP ツールを設定すること、内部サーバーとホストプロキシ経由サーバーの比較。

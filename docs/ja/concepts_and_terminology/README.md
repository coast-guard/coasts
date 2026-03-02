# 概念と用語

このセクションでは、Coasts 全体で使用される中核となる概念と語彙を扱います。Coasts が初めての場合は、設定や高度な使い方に入る前に、ここから始めてください。

- [Coasts](COASTS.md) — プロジェクトの自己完結型ランタイム。それぞれが独自のポート、ボリューム、worktree の割り当てを持ちます。
- [Filesystem](FILESYSTEM.md) — ホストと Coast 間の共有マウント、ホスト側エージェント、そして worktree の切り替え。
- [Coast Daemon](DAEMON.md) — ライフサイクル操作を実行するローカルの `coastd` コントロールプレーン。
- [Coast CLI](CLI.md) — コマンド、スクリプト、エージェントのワークフローのためのターミナル・インターフェース。
- [Coastguard](COASTGUARD.md) — 可観測性と制御のために `coast ui` で起動する Web UI。
- [Ports](PORTS.md) — 正規ポートと動的ポート、および checkout がそれらの間でどのようにスワップするか。
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — プライマリサービスへのクイックリンク、Cookie 分離のためのサブドメインルーティング、URL テンプレート。
- [Assign and Unassign](ASSIGN.md) — Coast を worktree 間で切り替える方法と、利用可能な assign 戦略。
- [Checkout](CHECKOUT.md) — 正規ポートを Coast インスタンスにマッピングすること、およびそれが必要になるタイミング。
- [Lookup](LOOKUP.md) — エージェントの現在の worktree に一致する Coast インスタンスを発見する方法。
- [Volume Topology](VOLUMES.md) — 共有サービス、共有ボリューム、分離ボリューム、スナップショット。
- [Shared Services](SHARED_SERVICES.md) — ホスト管理のインフラサービスとボリュームの識別。
- [Secrets and Extractors](SECRETS.md) — ホストのシークレットを抽出し、Coast コンテナへ注入する。
- [Builds](BUILDS.md) — coast build の構造、成果物の保存場所、自動プルーニング、型付きビルド。
- [Coastfile Types](COASTFILE_TYPES.md) — extends、unset、omit、autostart を備えた合成可能な Coastfile バリアント。
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — DinD ランタイム、Docker-in-Docker アーキテクチャ、そしてサービスが Coast 内でどのように実行されるか。
- [Bare Services](BARE_SERVICES.md) — Coast 内で非コンテナ化プロセスを実行すること、および代わりにコンテナ化すべき理由。
- [Logs](LOGS.md) — Coast 内からサービスログを読む方法、MCP のトレードオフ、Coastguard のログビューア。
- [Exec & Docker](EXEC_AND_DOCKER.md) — Coast 内でコマンドを実行し、内側の Docker デーモンと通信する。
- [Agent Shells](AGENT_SHELLS.md) — コンテナ化されたエージェント TUI、OAuth のトレードオフ、そしておそらくエージェントはホスト上で実行すべき理由。
- [MCP Servers](MCP_SERVERS.md) — コンテナ化されたエージェントのために Coast 内で MCP ツールを設定すること、内部サーバーとホストプロキシされたサーバー。
- [Troubleshooting](TROUBLESHOOTING.md) — doctor、デーモン再起動、プロジェクト削除、そして factory-reset の全消去オプション。

# コンセプトと用語

このセクションでは、Coasts 全体で使用される中核となるコンセプトと用語を扱います。Coasts を初めて使う場合は、設定や高度な使い方に入る前に、まずここから始めてください。

- [Coasts](COASTS.md) - プロジェクトの自己完結型ランタイムであり、それぞれが独自のポート、ボリューム、worktree の割り当てを持ちます。
- [Run](RUN.md) - 最新のビルドから新しい Coast インスタンスを作成し、必要に応じて worktree を割り当てます。
- [Remove](REMOVE.md) - クリーンに再作成したい場合や Coasts を停止したい場合に、Coast インスタンスとその分離されたランタイム状態を削除します。
- [Filesystem](FILESYSTEM.md) - ホストと Coast 間の共有マウント、ホスト側エージェント、および worktree の切り替えです。
- [Private Paths](PRIVATE_PATHS.md) - 共有 bind mount 間で競合するワークスペースパスに対する、インスタンスごとの分離です。
- [Coast Daemon](DAEMON.md) - ライフサイクル操作を実行するローカルの `coastd` コントロールプレーンです。
- [Coast CLI](CLI.md) - コマンド、スクリプト、エージェントワークフローのためのターミナルインターフェースです。
- [Coastguard](COASTGUARD.md) - 可観測性と制御のために `coast ui` で起動される Web UI です。
- [Ports](PORTS.md) - 正式ポートと動的ポート、および checkout がそれらの間をどのように切り替えるかです。
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - 主要サービスへのクイックリンク、Cookie 分離のためのサブドメインルーティング、および URL テンプレートです。
- [Assign and Unassign](ASSIGN.md) - Coast を worktree 間で切り替えること、および利用可能な assign 戦略です。
- [Checkout](CHECKOUT.md) - 正式ポートを Coast インスタンスにマッピングすることと、それが必要になる場面です。
- [Lookup](LOOKUP.md) - エージェントの現在の worktree に一致する Coast インスタンスを見つけます。
- [Volume Topology](VOLUMES.md) - 共有サービス、共有ボリューム、分離ボリューム、およびスナップショットです。
- [Shared Services](SHARED_SERVICES.md) - ホスト管理のインフラサービスとボリュームの曖昧性解消です。
- [Secrets and Extractors](SECRETS.md) - ホストのシークレットを抽出し、それらを Coast コンテナに注入します。
- [Builds](BUILDS.md) - coast build の構造、アーティファクトの保存場所、自動 pruning、および型付きビルドです。
- [Coastfile Types](COASTFILE_TYPES.md) - extends、unset、omit、および autostart を備えた、組み合わせ可能な Coastfile バリアントです。
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) - DinD ランタイム、Docker-in-Docker アーキテクチャ、およびサービスが Coast 内でどのように実行されるかです。
- [Bare Services](BARE_SERVICES.md) - Coast 内で非コンテナ化プロセスを実行することと、代わりにコンテナ化すべき理由です。
- [Bare Service Optimization](BARE_SERVICE_OPTIMIZATION.md) - 条件付きインストール、キャッシュ、private_paths、共有サービス接続性、および bare service のための assign 戦略です。
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 自動注入される `<SERVICE>_DYNAMIC_PORT` 変数と、それらをサービスコマンドで使用する方法です。
- [Logs](LOGS.md) - Coast 内からサービスログを読むこと、MCP のトレードオフ、および Coastguard のログビューアです。
- [Exec & Docker](EXEC_AND_DOCKER.md) - Coast 内でコマンドを実行することと、内部 Docker デーモンとやり取りすることです。
- [Agent Shells](AGENT_SHELLS.md) - コンテナ化されたエージェント TUI、OAuth のトレードオフ、および代わりにホスト上でエージェントを実行したほうがよい理由です。
- [MCP Servers](MCP_SERVERS.md) - コンテナ化されたエージェント向けに Coast 内で MCP ツールを設定すること、内部サーバーとホスト経由プロキシサーバーの違いです。
- [Remotes](REMOTES.md) - ローカルワークフローを変更せずに、`coast-service` 経由でリモートマシン上でサービスを実行します。
- [Troubleshooting](TROUBLESHOOTING.md) - doctor、daemon の再起動、プロジェクトの削除、および factory-reset の最終手段オプションです。

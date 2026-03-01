# Coasts ドキュメント

## インストール

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install` を実行しない場合、毎回必ず `coast daemon start` でデーモンを手動で起動する責任はあなたにあります。*

## Coasts とは？

Coast（**コンテナ化されたホスト**）は、ローカル開発用のランタイムです。Coasts を使うと、同一プロジェクトに対して 1 台のマシン上で複数の分離された環境を実行できます。

Coasts は、多数の相互依存サービスを含む複雑な `docker-compose` スタックで特に有用ですが、コンテナ化されていないローカル開発セットアップにも同等に効果的です。Coasts は幅広い [ランタイム構成パターン](concepts_and_terminology/RUNTIMES_AND_SERVICES.md) をサポートしており、並行して作業する複数のエージェントにとって理想的な環境を形作れます。

Coasts はホスト型クラウドサービスとしてではなく、ローカル開発のために作られています。環境はあなたのマシン上でローカルに動作します。

Coasts プロジェクトは、無料・ローカル・MIT ライセンス・エージェントプロバイダ非依存・エージェントハーネス非依存のソフトウェアで、AI のアップセルはありません。

Coasts は、worktree を使用するあらゆるエージェント的コーディングワークフローで動作します。ハーネス側の特別な設定は不要です。

## Worktrees に Coasts を使う理由

Git の worktree はコード変更の分離に優れていますが、ランタイムの分離そのものは解決しません。

複数の worktree を並行実行すると、すぐに操作性の問題に突き当たります:

- 同じホストポートを想定するサービス間の [ポート競合](concepts_and_terminology/PORTS.md)。
- worktree ごとのデータベースおよび [ボリューム設定](concepts_and_terminology/VOLUMES.md) が管理しづらく、面倒。
- worktree ごとにカスタムのランタイム配線が必要な統合テスト環境。
- worktree を切り替えるたびにランタイムコンテキストを再構築するという生き地獄。[Assign and Unassign](concepts_and_terminology/ASSIGN.md) を参照。

Git がコードのバージョン管理だとすれば、Coasts は worktree ランタイムのための Git のようなものです。

各環境には専用のポートが割り当てられるため、どの worktree ランタイムも並行して調査できます。[チェックアウト](concepts_and_terminology/CHECKOUT.md) して worktree ランタイムを切り替えると、Coasts はそのランタイムをプロジェクトの正規ポートに再マップします。

Coasts はランタイム構成を、worktree の上に載るシンプルでモジュール化されたレイヤーへ抽象化します。そのため、複雑な worktree 別セットアップを手作業で維持することなく、各 worktree を必要な分離度で動かせます。

## 要件

- macOS
- Docker Desktop
- Git を使用しているプロジェクト
- Node.js
- `socat` *(Homebrew の `depends_on` 依存として `curl -fsSL https://coasts.dev/install | sh` でインストールされます)*

```text
Linux 注記: Coasts はまだ Linux 上でテストしていませんが、Linux 対応は計画されています。
現時点でも Linux で Coasts を動かしてみることはできますが、正しく動作する保証は提供しません。
```

## エージェントをコンテナ化する？

Coast でエージェントをコンテナ化できます。最初は良いアイデアに聞こえるかもしれませんが、多くの場合、コーディングエージェントを実際にコンテナ内で動かす必要はありません。

Coasts は共有ボリュームマウントを通じてホストマシンと [ファイルシステム](concepts_and_terminology/FILESYSTEM.md) を共有するため、最も簡単で信頼性の高いワークフローは、エージェントをホスト上で動かし、（統合テストなどの）ランタイム負荷の高いタスクを [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md) を使って Coast インスタンス内で実行するよう指示することです。

ただし、エージェントをコンテナ内で動かしたい場合も、Coasts は [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md) を通じてそれを完全にサポートします。[MCP サーバー構成](concepts_and_terminology/MCP_SERVERS.md) を含む、このセットアップ向けの非常に入り組んだリグを構築できますが、現時点で存在するオーケストレーションソフトウェアとはきれいに相互運用できない可能性があります。多くのワークフローでは、ホスト側エージェントのほうがシンプルで信頼性が高いです。

## Coasts と Dev Containers の違い

Coasts は dev container ではなく、同じものでもありません。

Dev container は一般に、IDE を 1 つのコンテナ化された開発ワークスペースへマウントする目的で設計されています。Coasts はヘッドレスで、worktree を用いた並行エージェント利用のための軽量環境として最適化されています — 複数の分離された worktree 対応ランタイム環境を並べて動かし、高速なチェックアウト切り替えと、インスタンスごとのランタイム分離制御を提供します。

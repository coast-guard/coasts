# Coasts ドキュメント

## インストール

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install` を実行しない場合、毎回 `coast daemon start` でデーモンを手動で起動する責任はあなたにあります。*

## Coasts とは？

Coast（**コンテナ化されたホスト**）は、ローカル開発用のランタイムです。Coasts を使うと、1台のマシン上で同じプロジェクトに対して複数の分離された環境を実行できます。

Coasts は、多くの相互依存サービスを含む複雑な `docker-compose` スタックに特に有用ですが、コンテナ化されていないローカル開発セットアップにも同様に効果的です。Coasts は幅広い[ランタイム設定パターン](concepts_and_terminology/RUNTIMES_AND_SERVICES.md)をサポートしているため、並行して作業する複数のエージェントにとって理想的な環境を形作れます。

Coasts はローカル開発のために作られており、ホストされたクラウドサービスではありません。環境はあなたのマシン上でローカルに動作します。

Coasts プロジェクトは無料でローカル動作し、MIT ライセンスで、エージェントプロバイダ非依存かつエージェントハーネス非依存のソフトウェアで、AI のアップセルはありません。

Coasts は worktree を使うあらゆるエージェント型コーディングワークフローで動作します。ハーネス側の特別な設定は不要です。

## Worktrees に Coasts を使う理由

Git worktree はコード変更の分離に優れていますが、それ自体ではランタイムの分離は解決しません。

複数の worktree を並行して動かすと、すぐに使い勝手の問題に突き当たります:

- 同じホストポートを想定するサービス間での[ポート競合](concepts_and_terminology/PORTS.md)。
- worktree ごとのデータベースおよび[ボリューム設定](concepts_and_terminology/VOLUMES.md)が面倒で管理が大変。
- worktree ごとにカスタムのランタイム配線が必要な統合テスト環境。
- worktree を切り替えるたびにランタイムコンテキストを再構築するという生き地獄。[Assign and Unassign](concepts_and_terminology/ASSIGN.md) を参照。

Git がコードのためのバージョン管理だとすれば、Coasts は worktree ランタイムのための Git のようなものです。

各環境には専用のポートが割り当てられるため、どの worktree ランタイムも並行して検査できます。[チェックアウト](concepts_and_terminology/CHECKOUT.md)で worktree ランタイムを切り替えると、Coasts はそのランタイムをプロジェクトの標準（カノニカル）ポートへリマップします。

Coasts はランタイム設定を worktree の上にあるシンプルでモジュール式のレイヤーへ抽象化するため、複雑な worktree ごとのセットアップを手作業で保守することなく、各 worktree を必要な分離度で実行できます。

## 要件

- macOS
- Docker Desktop
- Git を使用するプロジェクト
- Node.js
- `socat` *(`curl -fsSL https://coasts.dev/install | sh` で Homebrew の `depends_on` 依存としてインストールされます)*

```text
Linux に関する注意: まだ Linux 上で Coasts をテストしていませんが、Linux 対応は計画されています。
現時点でも Linux で Coasts を動かすことは試せますが、正しく動作する保証は提供していません。
```

## エージェントをコンテナ化する？

Coast を使ってエージェントをコンテナ化できます。最初は素晴らしいアイデアに聞こえるかもしれませんが、多くの場合、コーディングエージェントをコンテナ内で実行する必要は実はありません。

Coasts は共有ボリュームマウントを通じてホストマシンと[ファイルシステム](concepts_and_terminology/FILESYSTEM.md)を共有するため、最も簡単で信頼性の高いワークフローは、エージェントをホスト上で実行し、統合テストのようなランタイム負荷の高いタスクを Coast インスタンス内で [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md) を使って実行するよう指示することです。

ただし、エージェントをコンテナ内で実行したい場合でも、Coasts は [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md) を通じて確実にそれをサポートします。[MCP サーバー設定](concepts_and_terminology/MCP_SERVERS.md)を含む、このセットアップのための非常に入り組んだリグを構築できますが、現時点で存在するオーケストレーションソフトウェアとはきれいに相互運用できない可能性があります。ほとんどのワークフローでは、ホスト側エージェントの方がシンプルで信頼性が高いです。

## Coasts と Dev Containers の比較

Coasts は dev container ではなく、同じものではありません。

Dev container は一般に、IDE を単一のコンテナ化された開発ワークスペースにマウントするために設計されています。Coasts はヘッドレスで、worktree を使った並行エージェント利用のための軽量環境として最適化されています — 複数の分離された worktree 対応ランタイム環境が並んで動作し、高速なチェックアウト切り替えと、各インスタンスごとのランタイム分離制御を備えています。

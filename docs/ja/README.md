# Coasts ドキュメント

```youtube
MBGKSKau4sU
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## インストール

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install` を実行しないことにした場合、毎回必ず `coast daemon start` でデーモンを手動で起動する責任はあなたにあります。*

## Coasts とは？

A Coast（**containerized host**）は、ローカル開発用のランタイムです。Coasts を使うと、1 台のマシン上で同一プロジェクトの複数の分離された環境を実行できます。

Coasts は、多数の相互依存サービスを持つ複雑な `docker-compose` スタックで特に有用ですが、コンテナ化されていないローカル開発セットアップでも同様に効果的です。Coasts は幅広い[ランタイム構成パターン](concepts_and_terminology/RUNTIMES_AND_SERVICES.md)をサポートしているため、並列に作業する複数のエージェントにとって理想的な環境を形作れます。

Coasts はホスト型クラウドサービスではなく、ローカル開発のために作られています。環境はあなたのマシン上でローカルに動作します。

Coasts プロジェクトは無料でローカル、MIT ライセンス、エージェントプロバイダ非依存、エージェントハーネス非依存のソフトウェアで、AI のアップセルはありません。

Coasts は worktree を使うあらゆるエージェント型コーディングワークフローで動作します。ハーネス側の特別な設定は不要です。

## Worktrees に Coasts を使う理由

Git worktree はコード変更の分離に優れていますが、ランタイムの分離をそれだけで解決するものではありません。

複数の worktree を並行して動かすと、すぐに使い勝手の問題に突き当たります:

- 同じホストポートを想定するサービス間の[ポート競合](concepts_and_terminology/PORTS.md)。
- worktree ごとのデータベースと[ボリューム設定](concepts_and_terminology/VOLUMES.md)が管理しづらく面倒。
- worktree ごとにカスタムのランタイム配線が必要な統合テスト環境。
- worktree を切り替えるたびにランタイムコンテキストを作り直すという生き地獄。[Assign and Unassign](concepts_and_terminology/ASSIGN.md) を参照。

Git がコードのバージョン管理だとすれば、Coasts は worktree のランタイムに対する Git のようなものです。

各環境には専用のポートが割り当てられるため、どの worktree ランタイムも並行して確認できます。worktree ランタイムを[チェックアウト](concepts_and_terminology/CHECKOUT.md)すると、Coasts はそのランタイムをプロジェクトの標準（canonical）ポートへ再マッピングします。

Coasts はランタイム構成を worktree の上にあるシンプルでモジュール式のレイヤーへ抽象化するため、複雑な worktree ごとの設定を手作業で保守することなく、各 worktree が必要とする分離で動かせます。

## 要件

- macOS または Linux
- macOS では Docker Desktop、Linux では Compose プラグイン付き Docker Engine
- Git を使うプロジェクト
- Node.js
- `socat`（macOS では `brew install socat`、Ubuntu では `sudo apt install socat`）

```text
Linux note: Dynamic ports work out of the box on Linux.
If you need canonical ports below `1024`, see the checkout docs for the required host configuration.
```

## エージェントをコンテナ化する？

Coast を使ってエージェントをコンテナ化できます。最初は良いアイデアに聞こえるかもしれませんが、多くの場合、実際にはコーディングエージェントをコンテナ内で動かす必要はありません。

Coasts は共有ボリュームマウントを通じてホストマシンと[ファイルシステム](concepts_and_terminology/FILESYSTEM.md)を共有するため、最も簡単で信頼性の高いワークフローは、ホスト上でエージェントを動かし、統合テストのようなランタイム負荷の高いタスクを [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md) を使って Coast インスタンス内で実行するよう指示することです。

ただし、エージェントをコンテナ内で動かしたい場合でも、Coasts は[Agent Shells](concepts_and_terminology/AGENT_SHELLS.md) によってそれを全面的にサポートします。[MCP サーバー設定](concepts_and_terminology/MCP_SERVERS.md)を含む、このセットアップのための非常に入り組んだリグを構築できますが、現時点で存在するオーケストレーションソフトウェアとはきれいに相互運用できない可能性があります。ほとんどのワークフローでは、ホスト側エージェントの方がシンプルで信頼性が高いです。

## Coasts vs Dev Containers

Coasts は dev container ではなく、同じものではありません。

Dev container は一般に、IDE を 1 つのコンテナ化された開発ワークスペースへマウントするために設計されています。Coasts はヘッドレスで、worktree と並列エージェント利用のための軽量環境として最適化されています。つまり、worktree を認識する複数の分離されたランタイム環境を並べて実行でき、素早いチェックアウト切り替えと、インスタンスごとのランタイム分離制御を提供します。

## Demo Repo

Coasts を試すための小さなサンプルプロジェクトが欲しい場合は、[`coasts-demo` repository](https://github.com/coast-guard/coasts-demo)から始めてください。

## Coasts Video Course

動画の方がよければ、[Coasts Video Course](learn-coasts-videos/README.md) で各コアコンセプトをそれぞれ 3 分未満で学べます。

# トラブルシューティング

Coasts の問題の多くは、古い状態、孤立した Docker リソース、または同期がずれたデーモンに起因します。このページでは、軽度から最終手段までのエスカレーション手順を説明します。

## Doctor

何かがおかしいと感じる場合 — インスタンスが実行中と表示されるのに応答がない、ポートが詰まっているように見える、または UI が古いデータを表示している — まずは `coast doctor` から始めてください:

```bash
coast doctor
```

Doctor は状態データベースと Docker をスキャンし、不整合を検出します: コンテナが欠落している孤立したインスタンスレコード、状態レコードのない宙ぶらりんのコンテナ、実際には死んでいるのに実行中とマークされた共有サービス。見つかったものは自動的に修復します。

何も変更せずに、実行した場合に何をするかをプレビューするには:

```bash
coast doctor --dry-run
```

## Daemon Restart

デーモン自体が応答しないように見える、または不正な状態にある疑いがある場合は、再起動してください:

```bash
coast daemon restart
```

これは穏やかなシャットダウンシグナルを送信し、デーモンの終了を待ってから、新しいプロセスを起動します。インスタンスと状態は保持されます。

## Removing a Single Project

問題が 1 つのプロジェクトに限定されている場合は、他に影響を与えずに、そのビルド成果物と関連する Docker リソースを削除できます:

```bash
coast rm-build my-project
```

これはプロジェクトの成果物ディレクトリ、Docker イメージ、ボリューム、コンテナを削除します。最初に確認を求めます。プロンプトをスキップするには `--force` を渡してください。

## Factory Reset with Nuke

他の方法では解決しない場合 — あるいは完全にクリーンな状態からやり直したい場合 — `coast nuke` はフルの工場出荷状態リセットを実行します:

```bash
coast nuke
```

これにより、次が実行されます:

1. `coastd` デーモンを停止する。
2. coast が管理する Docker コンテナを **すべて** 削除する。
3. coast が管理する Docker ボリュームを **すべて** 削除する。
4. coast が管理する Docker ネットワークを **すべて** 削除する。
5. coast の Docker イメージを **すべて** 削除する。
6. `~/.coast/` ディレクトリ全体（状態データベース、ビルド、ログ、シークレット、イメージキャッシュ）を削除する。
7. `~/.coast/` を再作成し、デーモンを再起動して、coast をすぐに再び使えるようにする。

これはすべてを破壊するため、確認プロンプトで `nuke` と入力する必要があります:

```text
$ coast nuke
WARNING: This will permanently destroy ALL coast data:

  - Stop the coastd daemon
  - Remove all coast-managed Docker containers
  - Remove all coast-managed Docker volumes
  - Remove all coast-managed Docker networks
  - Remove all coast Docker images
  - Delete ~/.coast/ (state DB, builds, logs, secrets, image cache)

Type "nuke" to confirm:
```

プロンプトをスキップするには `--force` を渡してください（スクリプトで便利です）:

```bash
coast nuke --force
```

nuke の後、coast は使用可能な状態になっています — デーモンは稼働しており、ホームディレクトリも存在します。あとはプロジェクトを再度 `coast build` して `coast run` するだけです。

## Reporting Bugs

上記のいずれでも解決しない問題に遭遇した場合は、報告時にデーモンログを含めてください:

```bash
coast daemon logs
```

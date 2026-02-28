# ホストエージェント向けスキル

Coasts を使用するプロジェクトで AI コーディングエージェント（Claude Code、Codex、Conductor、Cursor など）を使う場合、エージェントには Coast ランタイムとのやり取り方法を教えるスキルが必要です。これがないと、エージェントはファイルを編集できても、テストの実行、ログの確認、実行中の環境内で変更が機能しているかの検証方法が分かりません。

このガイドでは、そのスキルのセットアップ手順を説明します。

## なぜエージェントにこれが必要なのか

Coasts は、ホストマシンと Coast コンテナの間で [filesystem](concepts_and_terminology/FILESYSTEM.md) を共有します。エージェントはホスト上でファイルを編集し、Coast 内で稼働しているサービスはその変更を即座に反映します。しかし、エージェントには依然として次のことが必要です。

1. **作業対象の Coast インスタンスを特定する** — `coast lookup` が、エージェントの現在のディレクトリからこれを解決します。
2. **Coast 内でコマンドを実行する** — テスト、ビルド、その他のランタイムタスクは `coast exec` を介してコンテナ内で実行されます。
3. **ログを読み、サービス状態を確認する** — `coast logs` と `coast ps` により、エージェントはランタイムのフィードバックを得られます。

以下のスキルは、この3点すべてをエージェントに教えます。

## スキル

以下を、エージェントの既存のスキル／ルール／プロンプトファイルに追加してください。エージェントにすでにテスト実行や開発環境との連携に関する指示がある場合は、それらと並べて配置してください。これは、ランタイム操作に Coasts を使う方法をエージェントに教えるものです。

```text-copy
This project uses Coasts (containerized host) for isolated development environments.
Your code edits are automatically visible inside the running Coast — the filesystem
is shared between the host and the container.

=== ORIENTATION ===

Before running any runtime commands, discover which Coast instance matches your
current working directory:

  coast lookup

This prints the instance name, ports, URLs, and example commands. Use the instance
name from the output for all subsequent commands.

If you need deeper context on how Coasts work, read these docs:

  coast docs --path concepts_and_terminology/LOOKUP.md
  coast docs --path concepts_and_terminology/FILESYSTEM.md
  coast docs --path concepts_and_terminology/EXEC_AND_DOCKER.md
  coast docs --path concepts_and_terminology/LOGS.md

=== RUNNING COMMANDS ===

Use `coast exec` to run commands inside the Coast. The shell starts at the workspace
root (where the Coastfile is). cd to your target directory first:

  coast exec <instance> -- sh -c "cd <dir> && <command>"

Examples:

  coast exec dev-1 -- sh -c "cd src && npm test"
  coast exec dev-1 -- sh -c "cd backend && go test ./..."
  coast exec dev-1 -- sh -c "cd apps/web && npx playwright test"

=== RUNTIME FEEDBACK ===

Check service status:

  coast ps <instance>

Read service logs:

  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

=== TROUBLESHOOTING ===

If you encounter errors or unfamiliar behavior, search the Coast docs:

  coast search-docs "error message or description"

This uses semantic search — describe the problem in natural language and it will
find the relevant documentation.

=== RULES ===

- Always run `coast lookup` before your first runtime command in a session.
- Do not run services directly on the host. Use `coast exec` for all runtime tasks.
- File edits on the host are instantly visible inside the Coast. You do not need
  to copy files or rebuild after editing.
- If `coast lookup` returns no instances, the Coast may not be running. Suggest
  `coast run dev-1` or check `coast ls` for the project state.
```

## エージェントにスキルを追加する

追加方法は、エージェントによって異なります。

### Claude Code

スキル本文をプロジェクトの `CLAUDE.md` ファイルに追加するか、専用のセクションを作成して追加してください。

### Codex

スキル本文をプロジェクトの `AGENTS.md` ファイルに追加してください。

### Cursor

プロジェクトルートに `.cursor/rules/coast.mdc`（または `.cursor/rules/coast.md`）としてルールファイルを作成し、上記のスキル本文を貼り付けてください。

### その他のエージェント

多くのエージェントは、プロジェクトレベルのプロンプトまたはルールファイルを何らかの形でサポートしています。セッション開始時にエージェントが読み込むものに、スキル本文を貼り付けてください。

## さらに読む

- 完全な設定スキーマを学ぶには [Coastfiles documentation](coastfiles/README.md) を読む
- インスタンス管理のコマンドを学ぶには [Coast CLI](concepts_and_terminology/CLI.md) を参照する
- Coasts を観測・制御するための Web UI である [Coastguard](concepts_and_terminology/COASTGUARD.md) を探る
- Coasts の仕組みを全体像として理解するには [Concepts & Terminology](concepts_and_terminology/README.md) を参照する

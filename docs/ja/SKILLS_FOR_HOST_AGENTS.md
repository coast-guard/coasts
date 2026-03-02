# ホストエージェント向けスキル

Coasts を使用するプロジェクトで AI コーディングエージェント（Claude Code、Codex、Conductor、Cursor、または同様のもの）を使う場合、エージェントには Coast ランタイムとのやり取り方法を教えるスキルが必要です。これがないと、エージェントはファイルを編集できても、テストの実行、ログの確認、実行環境内で変更が動作しているかの検証方法がわかりません。

このガイドでは、そのスキルのセットアップ手順を説明します。

## なぜエージェントにこれが必要なのか

Coasts はホストマシンと Coast コンテナ間で [filesystem](concepts_and_terminology/FILESYSTEM.md) を共有します。エージェントはホスト上のファイルを編集し、実行中の Coast 内のサービスは変更を即座に反映します。しかし、エージェントはそれでも次のことが必要です:

1. **作業対象の Coast インスタンスを特定する** — `coast lookup` がエージェントの現在のディレクトリからこれを解決します。
2. **Coast 内でコマンドを実行する** — テスト、ビルド、その他のランタイムタスクは `coast exec` を介してコンテナ内で行われます。
3. **ログを読み、サービス状態を確認する** — `coast logs` と `coast ps` がエージェントにランタイムのフィードバックを提供します。

以下のスキルは、この3つすべてをエージェントに教えます。

## スキル

次の内容を、エージェントの既存のスキル／ルール／プロンプトファイルに追加してください。エージェントにすでにテスト実行や開発環境との連携に関する指示がある場合は、それらと並べて配置します — これは、ランタイム操作に Coasts を使う方法をエージェントに教えるものです。

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

## エージェントへのスキル追加

最も速い方法は、エージェントに自己セットアップさせることです。プロジェクトディレクトリから次のいずれかを実行してください:

```sh
# Claude Code
claude -p "$(coast skills-prompt)"

# Codex
codex "$(coast skills-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast skills-prompt)"
```

これにより、エージェントはスキル本文と、それを自分自身の設定ファイル（`CLAUDE.md`、`AGENTS.md`、`.cursor/rules/coast.md` など）に書き込むための指示を受け取ります。

### 手動セットアップ

自分でスキルを追加したい場合:

- **Claude Code:** プロジェクトの `CLAUDE.md` ファイルにスキル本文を追加します。
- **Codex:** プロジェクトの `AGENTS.md` ファイルにスキル本文を追加します。
- **Cursor:** プロジェクトルートに `.cursor/rules/coast.md` を作成し、スキル本文を貼り付けます。
- **Other agents:** エージェントが起動時に読み取るプロジェクトレベルのプロンプト／ルールファイルにスキル本文を貼り付けます。

## さらに読む

- 完全な設定スキーマを学ぶには [Coastfiles documentation](coastfiles/README.md) を読む
- インスタンス管理のための [Coast CLI](concepts_and_terminology/CLI.md) コマンドを学ぶ
- Coasts の観測と制御を行う Web UI である [Coastguard](concepts_and_terminology/COASTGUARD.md) を探索する
- Coasts の仕組み全体像については [Concepts & Terminology](concepts_and_terminology/README.md) を参照する

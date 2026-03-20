# ホストエージェント向けスキル

アプリが Coasts 内で動作している間にホスト上で AI コーディングエージェントを使う場合、通常、そのエージェントには Coast 固有のセットアップが 2 つ必要です。

1. ハーネスのプロジェクト指示ファイルまたはルールファイルにある、常時有効な Coast Runtime セクション
2. ハーネスがプロジェクトスキルをサポートしている場合の、`/coasts` のような再利用可能な Coast ワークフロースキル

1 つ目がないと、エージェントはファイルを編集しても `coast exec` を使うことを忘れます。
2 つ目がないと、Coast の割り当て、ログ、UI フローを毎回チャットで説明し直す必要があります。

このガイドでは、セットアップを具体的かつ Coast 固有のものに絞って説明します。つまり、どのファイルを作るか、そこにどんなテキストを入れるか、そしてそれがハーネスごとにどう変わるかを扱います。

## なぜエージェントにこれが必要なのか

Coasts は、ホストマシンと Coast コンテナの間で [filesystem](concepts_and_terminology/FILESYSTEM.md) を共有します。エージェントはホスト上でファイルを編集し、Coast 内で実行中のサービスはその変更を即座に認識します。しかし、エージェントは依然として次のことを行う必要があります。

1. 現在のチェックアウトに対応する Coast インスタンスを見つける
2. その Coast の中でテスト、ビルド、ランタイムコマンドを実行する
3. Coast からログとサービス状態を読む
4. まだ Coast がアタッチされていない場合に worktree の割り当てを安全に処理する

## 何をどこに置くか

- `AGENTS.md`、`CLAUDE.md`、または `.cursor/rules/coast.md` — スキルが呼び出されない場合でも、すべてのタスクに適用されるべき短い Coast ルール
- skill (`.agents/skills/...`、`.claude/skills/...`、または `.cursor/skills/...`) — `/coasts` のような、再利用可能な Coast ワークフロー本体
- command file (`.claude/commands/...` または `.cursor/commands/...`) — サポートするハーネス向けの任意の明示的エントリポイント。単純な選択肢の 1 つは、その command から skill を再利用することです

1 つのリポジトリで複数のハーネスを使う場合は、正本となる Coast スキルを 1 か所に置き、必要な場所でそれを公開してください。詳しくは
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md) を参照してください。

## 1. 常時有効な Coast Runtime ルール

次のブロックを、ハーネスの常時有効なプロジェクト指示ファイルまたはルールファイル（`AGENTS.md`、`CLAUDE.md`、`.cursor/rules/coast.md`、または同等のもの）に追加してください。

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

このブロックは常時有効ファイルに置くべきです。なぜなら、これらのルールは、エージェントが明示的に `/coasts` ワークフローに入ったときだけでなく、すべてのタスクに適用されるべきだからです。

## 2. 再利用可能な `/coasts` スキル

ハーネスがプロジェクトスキルをサポートしている場合は、スキル内容をスキルディレクトリ内の `SKILL.md` として保存してください。完全なスキルテキストは [skills_prompt.txt](skills_prompt.txt) にあります（CLI モードでは `coast skills-prompt` を使ってください）。Coast Runtime ブロックの後ろ、`---` フロントマターから始まる部分がスキル内容です。

Codex や OpenAI 固有のサーフェスを使っている場合は、表示メタデータや呼び出しポリシーのために、スキルの横に `agents/openai.yaml` を任意で追加できます。そのメタデータはスキルの横に置くべきであり、スキル自体の代わりにするべきではありません。

## ハーネス別クイックスタート

| Harness | Always-on file | Reusable Coast workflow | Notes |
|---------|----------------|-------------------------|-------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Coast ドキュメント向けに推奨する別個のプロジェクト command file はありません。[Codex](harnesses/CODEX.md) を参照してください。 |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md` は任意ですが、ロジックはスキル内に保ってください。[Claude Code](harnesses/CLAUDE_CODE.md) を参照してください。 |
| Cursor | `AGENTS.md` or `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` or shared `.agents/skills/coasts/SKILL.md` | `.cursor/commands/coasts.md` は任意です。`.cursor/worktrees.json` は Cursor の worktree ブートストラップ用であり、Coast ポリシー用ではありません。[Cursor](harnesses/CURSOR.md) を参照してください。 |
| Conductor | `CLAUDE.md` | Start with `CLAUDE.md`; use Conductor scripts and settings for Conductor-specific behavior | Claude Code の完全な project command 挙動を前提にしないでください。新しい command が表示されない場合は、Conductor を完全に閉じて再度開いてください。[Conductor](harnesses/CONDUCTOR.md) を参照してください。 |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | これはここで扱う中で最も制限の多いハーネスです。Codex スタイルのレイアウトを使い、Coast ドキュメントのために T3 ネイティブな command レイヤーを作らないでください。[T3 Code](harnesses/T3_CODE.md) を参照してください。 |

## エージェント自身にセットアップさせる

最も手早い方法は、エージェント自身に正しいファイルを書かせることです。以下のプロンプトをエージェントのチャットにコピーしてください。これには Coast Runtime ブロック、`coasts` スキルブロック、および各要素をどこに置くべきかというハーネス別の指示が含まれています。

```prompt-copy
skills_prompt.txt
```

CLI から `coast skills-prompt` を実行しても、同じ出力を取得できます。

## 手動セットアップ

- **Codex:** Coast Runtime セクションを `AGENTS.md` に置き、その後、再利用可能な `coasts` スキルを `.agents/skills/coasts/SKILL.md` に置いてください。
- **Claude Code:** Coast Runtime セクションを `CLAUDE.md` に置き、その後、再利用可能な `coasts` スキルを `.claude/skills/coasts/SKILL.md` に置いてください。`command file` が特に必要な場合にのみ `.claude/commands/coasts.md` を追加してください。
- **Cursor:** 最も移植性の高い指示にしたいなら Coast Runtime セクションを `AGENTS.md` に置き、Cursor ネイティブなプロジェクトルールにしたいなら `.cursor/rules/coast.md` に置いてください。再利用可能な `coasts` ワークフローは、Cursor 専用のリポジトリなら `.cursor/skills/coasts/SKILL.md` に、他のハーネスと共有するリポジトリなら `.agents/skills/coasts/SKILL.md` に置いてください。明示的な command file が特に必要な場合にのみ `.cursor/commands/coasts.md` を追加してください。
- **Conductor:** Coast Runtime セクションを `CLAUDE.md` に置いてください。Conductor 固有のブートストラップや実行挙動には、Conductor Repository Settings のスクリプトを使ってください。command を追加しても表示されない場合は、アプリを完全に閉じて再度開いてください。
- **T3 Code:** Codex と同じレイアウト、つまり `AGENTS.md` と `.agents/skills/coasts/SKILL.md` を使ってください。ここでは T3 Code を別個の Coast command サーフェスとしてではなく、薄い Codex スタイルのハーネスとして扱ってください。
- **Multiple harnesses:** 正本となるスキルは `.agents/skills/coasts/SKILL.md` に置いてください。Cursor はそれを直接読み込めます。必要であれば `.claude/skills/coasts/` 経由で Claude Code に公開してください。

## さらに読む

- ハーネスごとの対応表については [Harnesses guide](harnesses/README.md) を読んでください
- 共有レイアウトパターンについては [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md) を読んでください
- 完全な設定スキーマを学ぶには [Coastfiles documentation](coastfiles/README.md) を読んでください
- インスタンス管理用コマンドについては [Coast CLI](concepts_and_terminology/CLI.md) を参照してください
- Coasts を観察および制御するための Web UI である [Coastguard](concepts_and_terminology/COASTGUARD.md) を確認してください

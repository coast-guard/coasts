# 複数のハーネス

1 つのリポジトリが複数のハーネスから使われる場合、Coasts のセットアップを統合する 1 つの方法は、
共有の `/coasts` ワークフローを 1 か所に置き、ハーネス固有の常時有効ルールは各ハーネス用の
ファイルに保持することです。

## 推奨レイアウト

```text
AGENTS.md
CLAUDE.md
.cursor/rules/coast.md           # optional Cursor-native always-on rules
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md       # optional, thin, harness-specific
.claude/commands/coasts.md   # optional, thin, harness-specific
```

このレイアウトは次のように使います:

- `AGENTS.md` — Codex と T3
  Code で Coasts を扱うための短い常時有効ルール
- `.cursor/rules/coast.md` — オプションの Cursor ネイティブな常時有効ルール
- `CLAUDE.md` — Claude Code
  と Conductor で Coasts を扱うための短い常時有効ルール
- `.agents/skills/coasts/SKILL.md` — 正式な再利用可能 `/coasts` ワークフロー
- `.agents/skills/coasts/agents/openai.yaml` — オプションの Codex/OpenAI メタデータ
- `.claude/skills/coasts` — Claude Code
  でも同じスキルが必要な場合の、Claude 向けのミラーまたはシンボリックリンク
- `.cursor/commands/coasts.md` — オプションの Cursor コマンドファイル。単純な
  選択肢の 1 つは、これに同じスキルを再利用させることです
- `.claude/commands/coasts.md` — オプションの明示的なコマンドファイル。単純な
  選択肢の 1 つは、これに同じスキルを再利用させることです

## 手順

1. Coast Runtime ルールを常時有効の指示ファイルに入れます。
   - `AGENTS.md`、`CLAUDE.md`、または `.cursor/rules/coast.md` は、
     「すべてのタスク」でのルールに答えるべきです: まず `coast lookup` を実行する、
     `coast exec` を使う、`coast logs` でログを読む、一致がない場合は
     `coast assign` または `coast run` の前に確認する。
2. Coasts 用の正式な単一スキルを 1 つ作成します。
   - 再利用可能な `/coasts` ワークフローを `.agents/skills/coasts/SKILL.md` に置きます。
   - そのスキルの中で Coast CLI を直接使います: `coast lookup`、
     `coast ls`、`coast run`、`coast assign`、`coast unassign`、
     `coast checkout`、および `coast ui`。
3. ハーネスが別のパスを必要とする場所にだけ、そのスキルを公開します。
   - Codex、T3 Code、Cursor はすべて `.agents/skills/` を直接使えます。
   - Claude Code には `.claude/skills/` が必要なので、正式な
     スキルをその場所にミラーするかシンボリックリンクします。
4. 明示的な `/coasts` エントリポイントが欲しい場合にのみ、コマンドファイルを追加します。
   - `.claude/commands/coasts.md` または
     `.cursor/commands/coasts.md` を作成する場合、単純な選択肢の 1 つは、そのコマンドに
     同じスキルを再利用させることです。
   - コマンドに独自の別個の指示を与える場合、保守すべきワークフローの
     2 つ目のコピーを持つことになります。
5. Conductor 固有のセットアップは、スキルではなく Conductor に保持します。
   - Conductor 自体に属するブートストラップや実行動作には、
     Conductor Repository Settings スクリプトを使います。
   - Coasts のポリシーと `coast` CLI の使用は、`CLAUDE.md` と
     共有スキルに保持します。

## 具体的な `/coasts` の例

良い共有 `coasts` スキルは、次の 3 つの仕事を行うべきです:

1. `Use Existing Coast`
   - `coast lookup` を実行する
   - 一致が存在する場合は、`coast exec`、`coast ps`、および `coast logs` を使う
2. `Manage Assignment`
   - `coast ls` を実行する
   - `coast run`、`coast assign`、`coast unassign`、または
     `coast checkout` を提示する
   - 既存のスロットを再利用したり妨げたりする前に確認する
3. `Open UI`
   - `coast ui` を実行する

これが `/coasts` ワークフローにとって正しい場所です。常時有効ファイルには、
スキルが一度も呼び出されない場合でも適用されなければならない短いルールだけを
保持するべきです。

## シンボリックリンクのパターン

Claude Code に Codex、T3 Code、または Cursor と同じスキルを再利用させたい場合、
1 つの選択肢はシンボリックリンクです:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

チームがシンボリックリンクを使いたくない場合は、リポジトリにチェックインされた
ミラーでも問題ありません。主な目標は、コピー間の不要な乖離を避けることです。

## ハーネス固有の注意点

- Claude Code: プロジェクトスキルとオプションのプロジェクトコマンドはどちらも有効ですが、
  ロジックはスキル内に保持してください。
- Cursor: 短い Coast
  Runtime ルールには `AGENTS.md` または `.cursor/rules/coast.md` を使い、再利用可能なワークフローにはスキルを使い、
  `.cursor/commands` はオプションのままにしてください。
- Conductor: まず `CLAUDE.md` と Conductor スクリプトおよび設定として扱ってください。
  コマンドを追加しても表示されない場合は、再確認する前にアプリを完全に閉じてから開き直してください。
- T3 Code: これはここで最も薄いハーネスの表面です。Codex スタイルの
  `AGENTS.md` と `.agents/skills` パターンを使い、Coasts に関するドキュメントのために
  別個の T3 固有コマンドレイアウトを作らないでください。
- Codex: `AGENTS.md` は短く保ち、再利用可能なワークフローは
  `.agents/skills` に置いてください。

# Cursor

[Cursor](https://cursor.com/docs/agent/overview) は現在のチェックアウト内で直接作業でき、さらにその Parallel Agents 機能は `~/.cursor/worktrees/<project-name>/` 配下に git
worktree を作成することもできます。

Coasts に関するドキュメントでは、これは 2 つのセットアップケースがあることを意味します:

- 現在のチェックアウトで Cursor を使うだけであれば、Cursor 固有の
  `worktree_dir` エントリは不要です
- Cursor Parallel Agents を使う場合は、Coasts がそれらの worktree を検出して割り当てできるように、Cursor の worktree ディレクトリを
  `worktree_dir` に追加してください

## Setup

### Current checkout only

Cursor がすでに開いているチェックアウトを編集しているだけであれば、Coasts は
Cursor 固有の特別な worktree パスを必要としません。Coasts はそのチェックアウトを、他の任意のローカルリポジトリルートと同様に扱います。

### Cursor Parallel Agents

Parallel Agents を使用する場合は、`~/.cursor/worktrees/<project-name>` を
`worktree_dir` に追加してください:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

Cursor は各エージェントの worktree を、そのプロジェクトごとのディレクトリ配下に保存します。Coasts は実行時に `~` を展開し、そのパスを外部パスとして扱うため、バインドマウントを有効にするには既存のインスタンスを再作成する必要があります:

```bash
coast rm my-instance
coast build
coast run my-instance
```

Coastfile の変更後、worktree 一覧は即座に更新されますが、Cursor Parallel Agent の worktree への割り当てには、コンテナ内の外部バインドマウントが必要です。

## Where Coasts guidance goes

### `AGENTS.md` or `.cursor/rules/coast.md`

短く、常時有効な Coast Runtime ルールはここに置いてください:

- 最も移植性の高いプロジェクト指示にしたい場合は `AGENTS.md` を使ってください
- Cursor ネイティブのプロジェクトルールと設定 UI サポートが必要な場合は `.cursor/rules/coast.md` を使ってください
- 明確な理由がない限り、同じ Coast Runtime ブロックを両方に重複して置かないでください

### `.cursor/skills/coasts/SKILL.md` or shared `.agents/skills/coasts/SKILL.md`

再利用可能な `/coasts` ワークフローはここに置いてください:

- Cursor 専用のリポジトリであれば、`.cursor/skills/coasts/SKILL.md` は自然な配置先です
- 複数ハーネス対応のリポジトリであれば、正本となる skill は
  `.agents/skills/coasts/SKILL.md` に置いてください。Cursor はそれを直接読み込めます
- skill は実際の `/coasts` ワークフロー、すなわち `coast lookup`,
  `coast ls`, `coast run`, `coast assign`, `coast unassign`,
  `coast checkout`, および `coast ui` を担当するべきです

### `.cursor/commands/coasts.md`

Cursor はプロジェクトコマンドもサポートしています。Coasts に関するドキュメントでは、コマンドはオプションとして扱ってください:

- 明示的な `/coasts` エントリポイントが欲しい場合にのみコマンドを追加してください
- シンプルな選択肢の 1 つは、そのコマンドで同じ skill を再利用することです
- コマンドに独自の別個の指示を持たせる場合、保守すべきワークフローのコピーを 2 つ持つことになります

### `.cursor/worktrees.json`

`.cursor/worktrees.json` は、Coasts のポリシーではなく Cursor 自身の worktree ブートストラップのために使ってください:

- 依存関係のインストール
- `.env` ファイルのコピーまたはシンボリックリンク作成
- データベースマイグレーションやその他の一度限りのブートストラップ手順の実行

Coast Runtime ルールや Coast CLI ワークフローを
`.cursor/worktrees.json` に移さないでください。

## Example layout

### Cursor only

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # optional
.cursor/rules/coast.md            # optional alternative to AGENTS.md
.cursor/worktrees.json            # optional, for Parallel Agents bootstrap
```

### Cursor plus other harnesses

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # optional
```

## What Coasts does

- **Run** — `coast run <name>` は最新のビルドから新しい Coast インスタンスを作成します。`coast run <name> -w <worktree>` を使うと、Cursor の worktree を 1 ステップで作成して割り当てできます。[Run](../concepts_and_terminology/RUN.md) を参照してください。
- **Current checkout** — Cursor が開いたリポジトリ内で直接作業している場合、特別な Cursor 対応は不要です。
- **Bind mount** — Parallel Agents の場合、Coasts は
  `~/.cursor/worktrees/<project-name>` をコンテナ内の
  `/host-external-wt/{index}` にマウントします。
- **Discovery** — `git worktree list --porcelain` は引き続きリポジトリスコープであるため、Coasts は現在のプロジェクトに属する Cursor worktree のみを表示します。
- **Naming** — Cursor Parallel Agent の worktree は、Coasts の CLI および UI ではブランチ名で表示されます。
- **Assign** — `coast assign` は、Cursor worktree が選択されると、外部バインドマウントパスから `/workspace` を再マウントします。
- **Gitignored sync** — 絶対パスを使ってホストファイルシステム上で引き続き動作します。
- **Orphan detection** — Cursor が古い worktree をクリーンアップした場合、Coasts は不足している gitdir を検出し、必要に応じてそれらの割り当てを解除できます。

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.cursor/worktrees/my-app"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.claude/worktrees/` — Claude Code の worktree
- `~/.codex/worktrees/` — Codex の worktree
- `~/.cursor/worktrees/my-app/` — Cursor Parallel Agent の worktree

## Limitations

- Cursor Parallel Agents を使っていない場合は、たまたま Cursor で編集しているからという理由だけで
  `~/.cursor/worktrees/<project-name>` を追加しないでください。
- Coast Runtime ルールは、常時有効な 1 つの場所、つまり `AGENTS.md` または
  `.cursor/rules/coast.md` に置いてください。両方に重複させると乖離を招きます。
- 再利用可能な `/coasts` ワークフローは skill に置いてください。`.cursor/worktrees.json` は
  Cursor のブートストラップ用であり、Coasts のポリシー用ではありません。
- 1 つのリポジトリを Cursor、Codex、Claude Code、または T3 Code で共有する場合は、
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) の共有レイアウトを優先してください。

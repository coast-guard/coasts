# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) は `$CODEX_HOME/worktrees`（通常は `~/.codex/worktrees`）に worktree を作成します。各 worktree は `~/.codex/worktrees/a0db/project-name` のような不透明なハッシュのディレクトリ配下に存在し、detached HEAD で開始され、Codex の保持ポリシーに基づいて自動的にクリーンアップされます。

[Codex docs](https://developers.openai.com/codex/app/worktrees/) より:

> worktree が作成される場所を制御できますか？
> 現時点ではできません。Codex は一貫して管理できるように、`$CODEX_HOME/worktrees` 配下に worktree を作成します。

これらの worktree はプロジェクトルートの外側に存在するため、Coasts がそれらを検出してマウントするには明示的な
設定が必要です。

## Setup

`worktree_dir` に `~/.codex/worktrees` を追加します:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coasts は実行時に `~` を展開し、`~/` または `/` で始まるパスを外部として扱います。詳細は [Worktree Directories](../coastfiles/WORKTREE_DIR.md) を
参照してください。

`worktree_dir` を変更した後は、バインドマウントを有効にするために既存のインスタンスを**再作成**する必要があります:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree の一覧はすぐに更新されます（Coasts は新しい Coastfile を読み込みます）が、
Codex worktree への割り当てにはコンテナ内のバインドマウントが必要です。

## Where Coasts guidance goes

Coasts を扱うには、Codex のプロジェクト指示ファイルと共有スキルレイアウトを使用します:

- 短い Coast Runtime ルールは `AGENTS.md` に置く
- 再利用可能な `/coasts` ワークフローは `.agents/skills/coasts/SKILL.md` に置く
- Codex はそのスキルを `/coasts` コマンドとして表示する
- Codex 固有のメタデータを使う場合は、スキルの横の
  `.agents/skills/coasts/agents/openai.yaml` に置く
- Coasts に関するドキュメントのためだけに別のプロジェクトコマンドファイルを作らないこと。スキルが再利用可能な公開面です
- このリポジトリが Cursor や Claude Code も使う場合は、正規のスキルを
  `.agents/skills/` に置き、そこから公開します。詳細は
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) と
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) を参照してください。

たとえば、最小限の `.agents/skills/coasts/agents/openai.yaml` は次のようになります:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

これにより、Codex でそのスキルがよりわかりやすいラベルで表示され、`/coasts` が明示的な
コマンドになります。スキルが MCP
サーバーやその他の OpenAI 管理のツール配線も必要とする場合にのみ `dependencies.tools` を追加してください。

## What Coasts does

- **Run** -- `coast run <name>` は最新のビルドから新しい Coast インスタンスを作成します。`coast run <name> -w <worktree>` を使うと、Codex worktree の作成と割り当てを 1 ステップで行えます。詳細は [Run](../concepts_and_terminology/RUN.md) を参照してください。
- **Bind mount** -- コンテナ作成時に、Coasts は
  `~/.codex/worktrees` をコンテナ内の `/host-external-wt/{index}` にマウントします。
- **Discovery** -- `git worktree list --porcelain` はリポジトリスコープであるため、そのディレクトリに多くのプロジェクトの worktree が含まれていても、現在のプロジェクトに属する Codex worktree のみが表示されます。
- **Naming** -- Detached HEAD の worktree は外部ディレクトリ内での相対パス（`a0db/my-app`, `eca7/my-app`）として表示されます。ブランチベースの worktree はブランチ名として表示されます。
- **Assign** -- `coast assign` は外部バインドマウントパスから `/workspace` を再マウントします。
- **Gitignored sync** -- ホストファイルシステム上で絶対パスを使って実行されるため、バインドマウントなしでも動作します。
- **Orphan detection** -- git watcher は外部ディレクトリを
  再帰的にスキャンし、`.git` の gitdir ポインタでフィルタします。Codex が
  worktree を削除した場合、Coasts はインスタンスの割り当てを自動的に解除します。

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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

- `.claude/worktrees/` -- Claude Code（ローカル、特別な処理なし）
- `~/.codex/worktrees/` -- Codex（外部、バインドマウントされる）

## Limitations

- Codex はいつでも worktree をクリーンアップする可能性があります。Coasts の orphan detection は
  これを適切に処理します。

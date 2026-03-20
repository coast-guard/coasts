# Conductor

[Conductor](https://conductor.build/) は、並列の Claude Code エージェントを実行し、それぞれが独自の分離されたワークスペースを持ちます。ワークスペースは `~/conductor/workspaces/<project-name>/` に保存された git worktree です。各ワークスペースは名前付きブランチでチェックアウトされます。

これらの worktree はプロジェクトルートの外側に存在するため、Coasts がそれらを検出してマウントするには明示的な
設定が必要です。

## Setup

`~/conductor/workspaces/<project-name>` を `worktree_dir` に追加します。Codex（すべてのプロジェクトを 1 つのフラットなディレクトリ配下に保存する）とは異なり、Conductor は worktree をプロジェクトごとのサブディレクトリ配下にネストするため、パスにはプロジェクト名を含める必要があります。以下の例では、`my-app` はあなたのリポジトリに対応する `~/conductor/workspaces/` 配下の実際のフォルダ名と一致している必要があります。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor ではリポジトリごとにワークスペースパスを設定できるため、デフォルトの `~/conductor/workspaces` があなたの設定と一致しない場合があります。実際のパスを確認するには Conductor のリポジトリ設定を確認し、それに応じて調整してください — ディレクトリがどこにあっても原則は同じです。

Coasts は実行時に `~` を展開し、`~/` または `/` で始まるパスを
外部として扱います。詳細は [Worktree Directories](../coastfiles/WORKTREE_DIR.md) を参照して
ください。

`worktree_dir` を変更した後は、bind mount を有効にするために既存のインスタンスを**再作成**する必要があります。

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree の一覧はすぐに更新されます（Coasts は新しい Coastfile を読み込みます）が、
Conductor worktree への割り当てにはコンテナ内の bind mount が必要です。

## Where Coasts guidance goes

Conductor は、Coasts と連携するための独自のハーネスとして扱ってください。

- 短い Coast Runtime のルールは `CLAUDE.md` に置く
- セットアップや実行時の挙動のうち、実際に Conductor 固有のものは
  Conductor Repository Settings のスクリプトを使う
- ここでは Claude Code の完全な project command や project skill の挙動を前提にしない
- コマンドを追加しても表示されない場合は、再テストする前に
  Conductor を完全に閉じてから再度開く
- このリポジトリが他のハーネスも使っている場合は、
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) と
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) を参照し、
  共有の `/coasts` ワークフローを 1 か所に保つ方法を確認してください

## What Coasts does

- **Run** — `coast run <name>` は最新のビルドから新しい Coast インスタンスを作成します。`coast run <name> -w <worktree>` を使うと、Conductor worktree の作成と割り当てを 1 ステップで行えます。詳細は [Run](../concepts_and_terminology/RUN.md) を参照してください。
- **Bind mount** — コンテナ作成時に、Coasts は
  `~/conductor/workspaces/<project-name>` をコンテナ内の
  `/host-external-wt/{index}` にマウントします。
- **Discovery** — `git worktree list --porcelain` はリポジトリスコープであるため、現在のプロジェクトに属する worktree のみが表示されます。
- **Naming** — Conductor の worktree は名前付きブランチを使用するため、Coasts UI と CLI ではブランチ
  名で表示されます（例: `scroll-to-bottom-btn`）。1 つのブランチは
  同時に 1 つの Conductor ワークスペースでしかチェックアウトできません。
- **Assign** — `coast assign` は `/workspace` を外部 bind mount パスから再マウントします。
- **Gitignored sync** — ホストファイルシステム上で絶対パスを使って実行されるため、bind mount なしでも動作します。
- **Orphan detection** — git watcher は外部ディレクトリを
  再帰的にスキャンし、`.git` の gitdir ポインタでフィルタリングします。Conductor がワークスペースをアーカイブまたは
  削除した場合、Coasts はインスタンスの割り当てを自動的に解除します。

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = ["~/conductor/workspaces/my-app"]
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

- `~/conductor/workspaces/my-app/` — Conductor（外部、bind-mounted; `my-app` はあなたのリポジトリフォルダ名に置き換えてください）

## Conductor Env Vars

- Coasts 内のランタイム設定で Conductor 固有の環境変数（例:
  `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`）に依存するのは避けてください。Coasts はポート、ワークスペースパス、サービスディスカバリを
  独立して管理します — 代わりに Coastfile の `[ports]` と `coast exec` を使用してください。

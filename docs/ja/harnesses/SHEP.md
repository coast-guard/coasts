# Shep

## クイックセットアップ

[Coast CLI](../GETTING_STARTED.md) が必要です。Coasts を自動的にセットアップするには、このプロンプトを
エージェントのチャットにコピーしてください:

```prompt-copy
shep_setup_prompt.txt
```

CLI からスキルの内容を取得することもできます: `coast skills-prompt`。

セットアップ後、新しいスキルとプロジェクト
命令を有効にするために、**エディタを終了して再度開いてください**。

---

[Shep](https://shep-ai.github.io/cli/) は `~/.shep/repos/{hash}/wt/{branch-slug}` に worktree を作成します。ハッシュはリポジトリの絶対パスの SHA-256 の先頭 16 桁の 16 進文字であるため、リポジトリごとに決定的ですが中身はわかりにくくなっています。特定のリポジトリのすべての worktree は同じハッシュを共有し、`wt/{branch-slug}` サブディレクトリによって区別されます。

Shep CLI からは、`shep feat show <feature-id>` で worktree パスが表示されます。あるいは、
`ls ~/.shep/repos` でリポジトリごとのハッシュディレクトリを一覧表示できます。

ハッシュはリポジトリごとに異なるため、Coasts はユーザーがハッシュをハードコードしなくても
shep worktree を検出できるように **glob パターン** を使用します。

## セットアップ

`worktree_dir` に `~/.shep/repos/*/wt` を追加します:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

`*` はリポジトリごとのハッシュディレクトリに一致します。実行時に Coasts は glob を展開し、
一致するディレクトリ（例: `~/.shep/repos/a21f0cda9ab9d456/wt`）を見つけて、
それをコンテナに bind mount します。glob
パターンの詳細については、
[Worktree Directories](../coastfiles/WORKTREE_DIR.md) を参照してください。

`worktree_dir` を変更した後は、bind mount を有効にするために既存のインスタンスを **再作成** する必要があります:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree の一覧はすぐに更新されます（Coasts は新しい Coastfile を読み取るため）が、
Shep worktree への割り当てにはコンテナ内の bind mount が必要です。

## Coasts のガイダンスの配置先

Shep は内部で Claude Code をラップしているため、Claude Code の慣習に従ってください:

- 短い Coast Runtime ルールは `CLAUDE.md` に置く
- 再利用可能な `/coasts` ワークフローは `.claude/skills/coasts/SKILL.md` または
  共通の `.agents/skills/coasts/SKILL.md` に置く
- このリポジトリが他の harness も使用している場合は、
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) および
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) を参照してください

## Coasts が行うこと

- **実行** -- `coast run <name>` は最新のビルドから新しい Coast インスタンスを作成します。`coast run <name> -w <worktree>` を使用すると、Shep worktree の作成と割り当てを 1 ステップで行えます。[Run](../concepts_and_terminology/RUN.md) を参照してください。
- **Bind mount** -- コンテナ作成時に、Coasts は glob
  `~/.shep/repos/*/wt` を解決し、一致する各ディレクトリをコンテナ内の
  `/host-external-wt/{index}` にマウントします。
- **検出** -- `git worktree list --porcelain` はリポジトリスコープであるため、
  現在のプロジェクトに属する worktree のみが表示されます。
- **命名** -- Shep worktree は名前付きブランチを使用するため、Coasts UI および CLI では
  ブランチ名で表示されます（例: `feat-green-background`）。
- **割り当て** -- `coast assign` は外部 bind mount パスから `/workspace` を再マウントします。
- **Gitignore されたファイルの同期** -- ホストファイルシステム上で絶対パスを使って実行されるため、bind mount なしで動作します。
- **孤立検出** -- git watcher は外部ディレクトリを
  再帰的にスキャンし、`.git` gitdir ポインタでフィルタリングします。Shep が
  worktree を削除すると、Coasts はインスタンスの割り当てを自動的に解除します。

## 例

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
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

- `~/.shep/repos/*/wt` -- Shep（外部、glob 展開によって bind mount）

## Shep パス構造

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

重要なポイント:
- 同じリポジトリ = 毎回同じハッシュ（決定的であり、ランダムではない）
- 異なるリポジトリ = 異なるハッシュ
- パス区切り文字はハッシュ化の前に `/` に正規化される
- ハッシュは `shep feat show <feature-id>` または `ls ~/.shep/repos` で確認できる

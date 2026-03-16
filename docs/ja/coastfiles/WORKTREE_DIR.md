# Worktree ディレクトリ

`[coast]` の `worktree_dir` フィールドは、git worktree をどこに配置するかを制御します。Coast は git worktree を使用して、フルのリポジトリを複製することなく、各インスタンスに異なるブランチ上のコードベースの独自コピーを持たせます。

## 構文

`worktree_dir` は単一の文字列または文字列の配列を受け取ります:

```toml
# Single directory (default)
worktree_dir = ".worktrees"

# Multiple directories
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

省略した場合、デフォルトは `".worktrees"` です。

## パスタイプ

### 相対パス

`~/` または `/` で始まらないパスは、プロジェクトルートを基準に解決されます。これらが最も一般的で、特別な処理は不要です — これらはプロジェクトディレクトリ内にあり、標準の `/host-project` バインドマウントを通じて Coast コンテナ内で自動的に利用可能です。

```toml
worktree_dir = ".worktrees"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### チルダパス（外部）

`~/` で始まるパスはユーザーのホームディレクトリに展開され、**外部** worktree ディレクトリとして扱われます。Coast は、コンテナがそれらにアクセスできるように別個のバインドマウントを追加します。

```toml
worktree_dir = ["~/.codex/worktrees", ".worktrees"]
```

これは、OpenAI Codex のようにプロジェクトルート外に worktree を作成するツールと統合する方法です（Codex は常に `$CODEX_HOME/worktrees` に worktree を作成します）。

### 絶対パス（外部）

`/` で始まるパスも外部として扱われ、専用のバインドマウントを取得します。

```toml
worktree_dir = ["/shared/worktrees", ".worktrees"]
```

## 外部ディレクトリの動作

Coast が外部 worktree ディレクトリ（チルダパスまたは絶対パス）を検出すると、3 つのことが起こります:

1. **コンテナのバインドマウント** — コンテナ作成時（`coast run`）に、解決されたホストパスが `/host-external-wt/{index}` にバインドマウントされます。ここで `{index}` は `worktree_dir` 配列内の位置です。これにより、外部ファイルがコンテナ内からアクセス可能になります。

2. **プロジェクトのフィルタリング** — 外部ディレクトリには複数のプロジェクトの worktree が含まれている可能性があります。Coast は `git worktree list --porcelain`（本質的に現在のリポジトリにスコープされたもの）を使って、このプロジェクトに属する worktree のみを検出します。git watcher も、各 worktree の `.git` ファイルを読み取り、その `gitdir:` ポインタが現在のリポジトリに解決されることを確認することで所有関係を検証します。

3. **ワークスペースの再マウント** — 外部 worktree に対して `coast assign` すると、Coast は通常の `/host-project/{dir}/{name}` の代わりに、外部バインドマウントパスから `/workspace` を再マウントします。

## 外部 worktree の命名

ブランチがチェックアウトされている外部 worktree は、ローカル worktree と同様にブランチ名で表示されます。

**detached HEAD** 上の外部 worktree（Codex で一般的）は、外部ディレクトリ内での相対パスを使って表示されます。たとえば、`~/.codex/worktrees/a0db/coastguard-platform` にある Codex worktree は、UI と CLI では `a0db/coastguard-platform` として表示されます。

## `default_worktree_dir`

Coast が**新しい** worktree を作成する際に使用するディレクトリを制御します（たとえば、既存の worktree がないブランチを割り当てる場合）。デフォルトでは `worktree_dir` の最初のエントリです。

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

外部ディレクトリが新しい worktree の作成に使われることはありません — Coast は常にローカル（相対）ディレクトリに worktree を作成します。`default_worktree_dir` フィールドが必要なのは、デフォルト（最初のエントリ）を上書きしたい場合だけです。

## 例

### Codex 統合

OpenAI Codex は `~/.codex/worktrees/{hash}/{project-name}` に worktree を作成します。これらを Coast で可視化し、割り当て可能にするには:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

これを追加すると、Codex の worktree が checkout モーダルおよび `coast ls` の出力に表示されるようになります。Coast インスタンスを Codex worktree に割り当てて、そのコードをフル開発環境で実行できます。

注意: 外部ディレクトリを追加した後でバインドマウントを有効にするには、コンテナを再作成する必要があります（`coast run`）。既存インスタンスを再起動するだけでは不十分です。

### Claude Code 統合

Claude Code はプロジェクト内の `.claude/worktrees/` に worktree を作成します。これは相対パス（プロジェクトルート内）なので、他のローカル worktree ディレクトリと同様に動作します — 外部マウントは不要です:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### 3 つすべてを組み合わせる

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

## Live Coastfile 読み取り

Coastfile 内の `worktree_dir` への変更は、worktree の**一覧表示**には即座に反映されます（API と git watcher は、キャッシュされたビルドアーティファクトだけでなく、ディスク上の最新の Coastfile を読み取ります）。ただし、外部の**バインドマウント**はコンテナ作成時にのみ作成されるため、新しく追加した外部ディレクトリをマウント可能にするにはインスタンスを再作成する必要があります。

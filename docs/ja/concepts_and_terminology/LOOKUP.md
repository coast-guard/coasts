# Lookup

`coast lookup` は、呼び出し元の現在の作業ディレクトリに対してどの Coast インスタンスが稼働しているかを検出します。これはホスト側エージェントが自身の状況を把握するために最初に実行すべきコマンドです — 「ここでコードを編集しているが、どの Coast とやり取りすべきか？」

```bash
coast lookup
```

Lookup は、あなたが [worktree](ASSIGN.md) の中にいるのか、あるいはプロジェクトルートにいるのかを検出し、デーモンに対して一致するインスタンスを問い合わせ、ポート、URL、サンプルコマンド付きで結果を表示します。

## Why This Exists

ホスト上で動作する AI コーディングエージェント（Cursor、Claude Code、Codex など）は、[共有ファイルシステム](FILESYSTEM.md) を通じてファイルを編集し、実行時の操作には Coast CLI コマンドを呼び出します。しかしその前に、エージェントは基本的な問いに答える必要があります: **作業しているディレクトリに対応する Coast インスタンスはどれか？**

`coast lookup` がなければ、エージェントは `coast ls` を実行し、インスタンステーブル全体を解析し、どの worktree にいるかを特定し、照合しなければなりません。`coast lookup` はそれらを 1 ステップで行い、エージェントが直接利用できる構造化された出力を返します。

このコマンドは、Coast を使うエージェントワークフロー向けの任意のトップレベル SKILL.md、AGENTS.md、またはルールファイルに含めるべきです。これはエージェントが実行時コンテキストを発見するためのエントリポイントです。

## Output Modes

### Default (human-readable)

```bash
coast lookup
```

```text
Coast instances for worktree feature/oauth (my-app):

  dev-1  running  ★ checked out

  Primary URL:  http://dev-1.localhost:62217

  SERVICE              CANONICAL       DYNAMIC
  ★ web                3000            62217
    api                8080            63889
    postgres           5432            55681

  Examples (exec starts at the workspace root where your Coastfile is, cd to your target directory first):
    coast exec dev-1 -- sh -c "cd <dir> && <command>"
    coast logs dev-1 --service <service>
    coast ps dev-1
```

Examples セクションは、`coast exec` がワークスペースルート — Coastfile が存在するディレクトリ — から開始されることを、エージェント（および人間）に思い出させます。サブディレクトリでコマンドを実行するには、exec の中でそのディレクトリへ `cd` します。

### Compact (`--compact`)

インスタンス名の JSON 配列を返します。対象にするインスタンスがどれかだけを知りたいスクリプトやエージェントツール向けに設計されています。

```bash
coast lookup --compact
```

```text
["dev-1"]
```

同じ worktree 上に複数インスタンスがある場合:

```text
["dev-1","dev-2"]
```

一致がない場合:

```text
[]
```

### JSON (`--json`)

完全な構造化レスポンスを、整形済み JSON として返します。ポート、URL、ステータスを機械可読形式で必要とするエージェント向けに設計されています。

```bash
coast lookup --json
```

```json
{
  "project": "my-app",
  "worktree": "feature/oauth",
  "project_root": "/Users/dev/my-app",
  "instances": [
    {
      "name": "dev-1",
      "status": "Running",
      "checked_out": true,
      "branch": "feature/oauth",
      "primary_url": "http://dev-1.localhost:62217",
      "ports": [
        { "logical_name": "web", "canonical_port": 3000, "dynamic_port": 62217, "is_primary": true },
        { "logical_name": "api", "canonical_port": 8080, "dynamic_port": 63889, "is_primary": false }
      ]
    }
  ]
}
```

## How It Resolves

Lookup は現在の作業ディレクトリから上方向にたどって最も近い Coastfile を見つけ、その後、どの worktree にいるかを判定します:

1. cwd が `{project_root}/{worktree_dir}/{name}/...` の配下にある場合、lookup はその worktree に割り当てられたインスタンスを見つけます。
2. cwd がプロジェクトルート（または worktree の中ではない任意のディレクトリ）である場合、lookup は **worktree が割り当てられていない** — つまり依然としてプロジェクトルートを指している — インスタンスを見つけます。

つまり lookup はサブディレクトリからでも機能します。`my-app/.coasts/feature-oauth/src/api/` にいる場合でも、lookup は worktree を `feature-oauth` として解決します。

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | 1 つ以上の一致するインスタンスが見つかった |
| 1 | 一致するインスタンスがない（結果が空） |

これにより lookup はシェルの条件分岐で利用できます:

```bash
if coast lookup > /dev/null 2>&1; then
  coast exec dev-1 -- sh -c "cd src && npm test"
fi
```

## For Agent Workflows

典型的なエージェント統合パターン:

1. エージェントは worktree ディレクトリ内で作業を開始する。
2. エージェントは `coast lookup` を実行して、インスタンス名、ポート、URL、サンプルコマンドを検出する。
3. エージェントは以降のすべての Coast コマンドでインスタンス名を使用する: `coast exec`、`coast logs`、`coast ps`。

```text
┌─── Agent (host machine) ────────────────────────────┐
│                                                      │
│  1. coast lookup                                     │
│       → instance names, ports, URLs, examples        │
│  2. coast exec dev-1 -- sh -c "cd src && npm test"   │
│  3. coast logs dev-1 --service web --tail 50         │
│  4. coast ps dev-1                                   │
│                                                      │
└──────────────────────────────────────────────────────┘
```

エージェントが複数の worktree をまたいで作業している場合、各 worktree ディレクトリから `coast lookup` を実行して、それぞれのコンテキストに対して正しいインスタンスを解決します。

ホストエージェントが Coast とどのようにやり取りするかについては [Filesystem](FILESYSTEM.md) を、worktree の概念については [Assign and Unassign](ASSIGN.md) を、Coast 内でコマンドを実行する方法については [Exec & Docker](EXEC_AND_DOCKER.md) を参照してください。

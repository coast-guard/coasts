# Filesystem

ホストマシンと各 Coast インスタンスは同じプロジェクトファイルを共有します。ホストのプロジェクトルートは DinD コンテナ内の `/workspace` にバインドマウントされるため、ホストでの編集は Coast 内に即座に反映され、その逆も同様です。これにより、ホストマシン上で動作するエージェントがコードを編集しつつ、Coast 内のサービスがリアルタイムで変更を取り込めます。

## The Shared Mount

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

ホストのプロジェクトルートは、コンテナ作成時に [DinD コンテナ](RUNTIMES_AND_SERVICES.md) 内の `/host-project` に読み書き可能でマウントされます。コンテナ起動後、コンテナ内で `mount --bind /host-project /workspace` を実行して共有マウント伝播（`mount --make-rshared`）付きの作業用パス `/workspace` を作成するため、`/workspace` のサブディレクトリをバインドマウントする内側の compose サービスが正しい内容を参照できます。

この 2 段階の方式には理由があります。`/host-project` の Docker バインドマウントはコンテナ作成時に固定され、コンテナを作り直さない限り変更できません。一方、コンテナ内の `/workspace` に対する Linux のバインドマウントは、コンテナのライフサイクルに触れずにアンマウントして別のサブディレクトリ（worktree）へ再バインドできます。これが `coast assign` を高速にしている理由です。

`/workspace` は読み書き可能です。ファイル変更は即座に双方向へ流れます。ホストでファイルを保存すれば Coast 内の開発サーバーが変更を取り込みます。Coast 内でファイルを作成すればホスト側に現れます。

## Host Agents and Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

ファイルシステムが共有されているため、ホスト上で動作する AI コーディングエージェントは自由にファイルを編集でき、Coast 内で動作中のサービスは変更を即座に認識します。エージェントは Coast コンテナ内で動作する必要はなく、通常どおりホストから操作します。

エージェントがランタイム情報（ログ、サービス状態、テスト出力）を必要とする場合は、ホストから Coast CLI コマンドを呼び出します:

- サービス出力を見るには `coast logs dev-1 --service web --tail 50`（[Logs](LOGS.md) を参照）
- サービス状態を見るには `coast ps dev-1`（[Runtimes and Services](RUNTIMES_AND_SERVICES.md) を参照）
- Coast 内でコマンドを実行するには `coast exec dev-1 -- npm test`（[Exec & Docker](EXEC_AND_DOCKER.md) を参照）

これが基本的なアーキテクチャ上の利点です: **コード編集はホストで行い、実行環境は Coast にあり、共有ファイルシステムが両者を橋渡しします。** ホスト上のエージェントは、作業のために Coast の「内側」に入る必要がありません。

## Worktree Switching

`coast assign` が Coast を別の worktree に切り替えるとき、プロジェクトルートではなくその git worktree を指すように `/workspace` を再マウントします:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

worktree はホスト上の `{project_root}/.worktrees/{worktree_name}` に作成されます。`.worktrees` ディレクトリ名は Coastfile の `worktree_dir` で設定でき、`.gitignore` に含めるべきです。

コンテナ内では `/workspace` が遅延アンマウントされ、`/host-project/.worktrees/{branch_name}` にある worktree サブディレクトリへ再バインドされます。この再マウントは高速で、DinD コンテナを作り直したり内側の Docker デーモンを再起動したりしません。内側の compose サービスは再作成され、バインドマウントが新しい `/workspace` を通して解決されます。

`node_modules` のような gitignore 対象ファイルは、ハードリンク付きの rsync によりプロジェクトルートから worktree へ同期されるため、大きな依存関係ツリーでも初期セットアップはほぼ瞬時です。

macOS では、ホストと Docker VM 間のファイル I/O に固有のオーバーヘッドがあります。Coast は assign と unassign の際に `git ls-files` を実行して worktree の差分を取るため、大規模コードベースでは目立つ遅延が発生することがあります。プロジェクト内の一部（ドキュメント、テストフィクスチャ、スクリプトなど）が assign 間の差分対象である必要がない場合は、Coastfile の `exclude_paths` で除外してこのオーバーヘッドを減らせます。詳細は [Assign and Unassign](ASSIGN.md) を参照してください。

`coast unassign` は `/workspace` を `/host-project`（プロジェクトルート）に戻します。停止後に `coast start` すると、インスタンスに worktree が割り当てられているかどうかに応じて正しいマウントが再適用されます。

## All Mounts

各 Coast コンテナには次のマウントがあります:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | プロジェクトルートまたは worktree。assign 時に切り替え可能。 |
| `/host-project` | Docker bind mount | RW | 生のプロジェクトルート。コンテナ作成時に固定。 |
| `/image-cache` | Docker bind mount | RO | `~/.coast/image-cache/` からの事前に pull 済み OCI tarball。 |
| `/coast-artifact` | Docker bind mount | RO | 書き換え済み compose ファイルを含むビルド成果物。 |
| `/coast-override` | Docker bind mount | RO | [shared services](SHARED_SERVICES.md) 用に生成された compose override。 |
| `/var/lib/docker` | Named volume | RW | 内側の Docker デーモンの状態。コンテナ削除後も保持される。 |

読み取り専用マウントはインフラ用途で、Coast が生成するビルド成果物、キャッシュ済みイメージ、compose override を運びます。これらは `coast build` と Coastfile を通じて間接的に操作します。読み書き可能マウントは、あなたのコードが存在する場所であり、内側のデーモンが状態を保存する場所です。

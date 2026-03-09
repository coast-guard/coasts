# Filesystem

ホストマシンと各 Coast インスタンスは同じプロジェクトファイルを共有します。ホスト側のプロジェクトルートは DinD コンテナ内の `/host-project` に読み書き可能でマウントされ、Coast はアクティブな作業ツリーを `/workspace` にバインドマウントします。これにより、ホストマシン上で動作するエージェントがコードを編集しつつ、Coast 内のサービスが変更をリアルタイムで反映できるようになります。

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

ホスト側のプロジェクトルートは、コンテナ作成時に [DinD コンテナ](RUNTIMES_AND_SERVICES.md) 内の `/host-project` に読み書き可能でマウントされます。コンテナ起動後、コンテナ内で `mount --bind /host-project /workspace` を実行し、共有マウント伝播（`mount --make-rshared`）付きの作業用 `/workspace` パスを作成します。これにより、`/workspace` のサブディレクトリを bind-mount する内側の compose サービスは正しい内容を参照できます。

この二段階のアプローチには理由があります。`/host-project` の Docker bind mount はコンテナ作成時に固定され、コンテナを再作成しない限り変更できません。一方、コンテナ内の Linux bind mount である `/workspace` は、コンテナのライフサイクルに触れずにアンマウントして別のサブディレクトリ（worktree）へ再バインドできます。これが `coast assign` を高速にしている要因です。

`/workspace` は読み書き可能です。ファイル変更は両方向に即時反映されます。ホストでファイルを保存すると Coast 内の dev サーバーがそれを拾います。Coast 内でファイルを作成するとホストに表示されます。

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

ファイルシステムが共有されているため、ホスト上で動作する AI コーディングエージェントは自由にファイルを編集でき、Coast 内で稼働中のサービスは変更を即座に認識します。エージェントは Coast コンテナ内で動作する必要はなく、通常どおりホストから操作できます。

エージェントが実行時情報（ログ、サービス状態、テスト出力など）を必要とする場合は、ホストから Coast CLI コマンドを呼び出します。

- `coast logs dev-1 --service web --tail 50` でサービス出力を確認（[Logs](LOGS.md) を参照）
- `coast ps dev-1` でサービス状態を確認（[Runtimes and Services](RUNTIMES_AND_SERVICES.md) を参照）
- `coast exec dev-1 -- npm test` で Coast 内でコマンドを実行（[Exec & Docker](EXEC_AND_DOCKER.md) を参照）

これが基本となるアーキテクチャ上の利点です: **コード編集はホストで行い、実行は Coast で行い、共有ファイルシステムがそれらを橋渡しします。** ホスト側エージェントは作業のために Coast の「内側」に入る必要がありません。

## Worktree Switching

`coast assign` が Coast を別の worktree に切り替えると、プロジェクトルートではなくその git worktree を指すように `/workspace` を再マウントします。

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

worktree はホスト上の `{project_root}/.worktrees/{worktree_name}` に作成されます。`.worktrees` ディレクトリ名は Coastfile の `worktree_dir` で設定可能で、`.gitignore` に含めるべきです。

worktree が新規の場合、Coast は再マウントの前に、プロジェクトルートから選択した gitignored ファイルをブートストラップします。`git ls-files --others --ignored --exclude-standard` で無視ファイルを列挙し、一般的に重いディレクトリと設定された `exclude_paths` を除外してから、`rsync --files-from` と `--link-dest` を用いて選択ファイルを worktree へハードリンクします。Coast はそのブートストラップを内部 worktree メタデータに記録し、同一 worktree への後続の assign では、`coast assign --force-sync` で明示的に更新しない限りスキップします。

コンテナ内では、`/workspace` が遅延アンマウントされ、`/host-project/.worktrees/{branch_name}` にある worktree サブディレクトリへ再バインドされます。この再マウントは高速で、DinD コンテナの再作成や内側の Docker デーモン再起動は行いません。compose サービスおよび bare サービスは、新しい `/workspace` を経由して bind mount が解決されるよう、再マウント後に再作成または再起動されることがあります。

`node_modules` のような大きな依存ディレクトリは、この汎用ブートストラップ経路の対象ではありません。通常はサービス固有のキャッシュやボリュームによって扱います。

`[assign.rebuild_triggers]` を使用している場合、Coast はホスト上で `git diff --name-only <previous>..<worktree>` も実行し、`rebuild` とマークされたサービスを `restart` に格下げできるかどうかを判断します。assign のレイテンシに影響する詳細は [Assign and Unassign](ASSIGN.md) および [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) を参照してください。

`coast unassign` は `/workspace` を `/host-project`（プロジェクトルート）に戻します。停止後に `coast start` を実行すると、インスタンスに worktree が割り当てられているかどうかに基づいて、正しいマウントが再適用されます。

## All Mounts

すべての Coast コンテナには次のマウントがあります。

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | プロジェクトルートまたは worktree。assign で切り替え可能。 |
| `/host-project` | Docker bind mount | RW | 生のプロジェクトルート。コンテナ作成時に固定。 |
| `/image-cache` | Docker bind mount | RO | `~/.coast/image-cache/` からの事前 pull 済み OCI tarball。 |
| `/coast-artifact` | Docker bind mount | RO | 書き換え済み compose ファイルを含むビルド成果物。 |
| `/coast-override` | Docker bind mount | RO | [shared services](SHARED_SERVICES.md) 向けに生成された compose オーバーライド。 |
| `/var/lib/docker` | Named volume | RW | 内側の Docker デーモン状態。コンテナ削除後も永続化。 |

読み取り専用マウントはインフラです。Coast が生成するビルド成果物、キャッシュ済みイメージ、compose オーバーライドを運びます。これらは `coast build` と Coastfile を通じて間接的に操作します。読み書き可能マウントは、あなたのコードが存在する場所であり、内側のデーモンが状態を保存する場所です。

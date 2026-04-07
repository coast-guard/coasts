# ファイル同期

リモート coast は 2 層の同期戦略を使用します: 大量転送には rsync、継続的なリアルタイム同期には mutagen を使用します。どちらのツールも coast コンテナ内にインストールされる実行時依存関係であり、ホストマシンには不要です。

## 同期の実行場所

```text
Local Machine                          Remote Machine
┌─────────────────────────────┐        ┌──────────────────────────────┐
│  coastd daemon              │        │                              │
│    │                        │        │                              │
│    │ rsync (direct SSH)     │  SSH   │  /data/workspaces/{p}/{i}/   │
│    │────────────────────────│───────▶│    (rsync writes here)       │
│    │                        │        │    │                         │
│    │ docker exec            │        │    │ bind mount              │
│    ▼                        │        │    ▼                         │
│  Shell Container            │  SSH   │  Remote DinD Container       │
│    /workspace (bind mount)  │───────▶│    /workspace                │
│    mutagen (continuous sync)│        │    (compose services running)│
│    SSH key (copied in)      │        │                              │
└─────────────────────────────┘        └──────────────────────────────┘
```

デーモンはホストプロセスから直接 rsync を実行します。Mutagen はローカルの shell コンテナ内で `docker exec` によって実行されます。

## レイヤー 1: rsync（大量転送）

`coast run` と `coast assign` では、デーモンはホストから rsync を実行してワークスペースのファイルをリモートへ転送します:

```bash
rsync -rlDzP --delete-after \
  --rsync-path="sudo rsync" \
  --exclude '.git' --exclude 'node_modules' \
  --exclude 'target' --exclude '__pycache__' \
  --exclude '.react-router' --exclude '.next' \
  -e "ssh -p {port} -i {key}" \
  {local_workspace}/ {user}@{host}:{remote_workspace}/
```

rsync が完了した後、デーモンはリモートで `sudo chown -R` を実行し、SSH ユーザーがファイルの所有権を持つようにします。rsync は `--rsync-path="sudo rsync"` によって root として実行されます。これは、リモートワークスペースにコンテナ内の coast-service 操作によって作成された root 所有のファイルが含まれている可能性があるためです。

### rsync が得意なこと

- **初回転送。** 最初の `coast run` ではワークスペース全体が送信されます。
- **worktree の切り替え。** `coast assign` は古い worktree と新しい worktree の差分のみを送信します。変更されていないファイルは再送されません。
- **圧縮。** `-z` フラグは転送中のデータを圧縮します。

### 除外されるパス

rsync は転送すべきでないパスをスキップします:

| Path | Why |
|------|-----|
| `.git` | 大きく、リモートでは不要（worktree の内容だけで十分） |
| `node_modules` | ロックファイルから DinD 内で再構築される |
| `target` | Rust/Go のビルド成果物であり、リモートで再構築される |
| `__pycache__` | Python のバイトコードキャッシュであり、再生成される |
| `.react-router` | 生成された型であり、dev サーバーによって再作成される |
| `.next` | Next.js のビルドキャッシュであり、再生成される |

### 生成ファイルの保護

`coast assign` が `--delete-after` 付きで実行される場合、通常 rsync はローカルに存在しないファイルをリモートから削除します。これにより、リモートの dev サーバーが作成した生成ファイル（たとえば `generated/` の proto client など）が、ローカルの worktree に含まれていない場合に破壊されてしまいます。

これを防ぐため、rsync は `--filter 'P generated/***'` ルールを使用して、特定の生成ディレクトリが削除されないよう保護します。保護されるパスには `generated/`、`.react-router/`、`internal/generated/`、および `app/generated/` が含まれます。

### 部分転送の処理

rsync の終了コード 23（部分転送）は、致命的ではない警告として扱われます。これは、リモート DinD 内で動作している dev サーバーが rsync の書き込み中にファイル（例: `.react-router/types/`）を再生成する競合状態に対応するものです。ソースファイルの転送は成功し、失敗する可能性があるのは生成成果物のみですが、それらはいずれにせよ dev サーバーによって再生成されます。

## レイヤー 2: mutagen（継続的同期）

初回の rsync の後、デーモンはローカルの shell コンテナ内で mutagen セッションを開始します:

```bash
docker exec {shell_container} mutagen sync create \
    --name coast-{project}-{instance} \
    --sync-mode one-way-safe \
    --ignore-vcs \
    --ignore node_modules --ignore target \
    --ignore __pycache__ --ignore .next \
    /workspace/ {user}@{host}:{remote_workspace}/
```

Mutagen は OS レベルのイベント（コンテナ内では inotify）を通じてファイル変更を監視し、変更をバッチ化して、永続的な SSH 接続経由で差分を転送します。あなたの編集は数秒以内にリモートへ反映されます。

### one-way-safe モード

Mutagen は `one-way-safe` モードで動作します: 変更はローカルからリモートへのみ流れます。リモートで作成されたファイル（dev サーバー、ビルドツールなどによるもの）はローカルマシンへ同期されません。これにより、生成成果物が作業ディレクトリを汚染するのを防ぎます。

### Mutagen は実行時依存関係

Mutagen は次の場所にインストールされます:

- **coast image**（`[coast.setup]` から `coast build` によってビルドされる）: ローカルの shell コンテナで使用されます。
- **coast-service Docker image**（`Dockerfile.coast-service`）: リモート側で使用されます。

デーモンがホスト上で mutagen を直接実行することはありません。shell コンテナへ `docker exec` してオーケストレーションします。

## ライフサイクル

| Command | rsync | mutagen |
|---------|-------|---------|
| `coast run` | 初回の完全転送 | rsync 後にセッション作成 |
| `coast assign` | 新しい worktree の差分転送 | 古いセッションを終了し、新しいセッションを作成 |
| `coast stop` | -- | セッション終了 |
| `coast rm` | -- | セッション終了 |

### フォールバック動作

shell コンテナ内で mutagen セッションの開始に失敗した場合、デーモンは警告をログに記録します。初回の rsync によりワークスペースの内容自体は引き続き提供されますが、セッションが再確立されるまで（たとえば次回の `coast assign` やデーモン再起動時まで）ファイル変更はリアルタイムでは同期されません。

## 同期戦略の設定

Coastfile の `[remote]` セクションが同期戦略を制御します:

```toml
[remote]
workspace_sync = "mutagen"    # "rsync" (default) or "mutagen"
```

- **`rsync`**（デフォルト）: 初回の rsync 転送のみが実行されます。継続的同期はありません。リアルタイム同期が不要な CI 環境やバッチジョブに適しています。
- **`mutagen`**: 初回転送には rsync、その後の継続的同期には mutagen を使用します。編集内容をすぐにリモートへ反映させたい対話的な開発にはこちらを使ってください。

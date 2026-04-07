# 文件同步

远程 coast 使用双层同步策略:`rsync` 用于批量传输，`mutagen` 用于持续的实时同步。这两个工具都是运行时依赖，安装在 coast 容器内部——你的主机上不需要安装它们。

## 同步运行的位置

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

守护进程直接从主机进程运行 rsync。Mutagen 通过 `docker exec` 在本地 shell 容器内运行。

## 第 1 层:rsync（批量传输）

在 `coast run` 和 `coast assign` 时，守护进程从主机运行 rsync，将工作区文件传输到远端:

```bash
rsync -rlDzP --delete-after \
  --rsync-path="sudo rsync" \
  --exclude '.git' --exclude 'node_modules' \
  --exclude 'target' --exclude '__pycache__' \
  --exclude '.react-router' --exclude '.next' \
  -e "ssh -p {port} -i {key}" \
  {local_workspace}/ {user}@{host}:{remote_workspace}/
```

rsync 完成后，守护进程会在远端运行 `sudo chown -R`，将文件所有权赋予 SSH 用户。rsync 通过 `--rsync-path="sudo rsync"` 以 root 身份运行，因为远端工作区中可能包含来自容器内 coast-service 操作、由 root 拥有的文件。

### rsync 擅长的内容

- **初始传输。** 第一次 `coast run` 会发送整个工作区。
- **worktree 切换。** `coast assign` 只发送旧 worktree 与新 worktree 之间的增量。未变化的文件不会被重新传输。
- **压缩。** `-z` 标志会对传输中的数据进行压缩。

### 排除路径

rsync 会跳过不应传输的路径:

| Path | Why |
|------|-----|
| `.git` | 体积大，远端不需要（worktree 内容已足够） |
| `node_modules` | 在 DinD 内根据 lockfile 重新构建 |
| `target` | Rust/Go 构建产物，在远端重新构建 |
| `__pycache__` | Python 字节码缓存，会重新生成 |
| `.react-router` | 生成的类型，由开发服务器重新创建 |
| `.next` | Next.js 构建缓存，会重新生成 |

### 保护生成文件

当 `coast assign` 使用 `--delete-after` 运行时，rsync 通常会删除远端那些本地不存在的文件。这会破坏生成文件（例如位于 `generated/` 的 proto client），这些文件可能是远端开发服务器创建的，但你的本地 worktree 中并不包含它们。

为防止这种情况，rsync 使用 `--filter 'P generated/***'` 规则来保护特定生成目录不被删除。受保护的路径包括 `generated/`、`.react-router/`、`internal/generated/` 和 `app/generated/`。

### 部分传输处理

rsync 退出码 23（部分传输）被视为非致命警告。这用于处理一种竞争条件:当远端 DinD 中运行的开发服务器在 rsync 写入期间重新生成文件（例如 `.react-router/types/`）时，就可能发生这种情况。源文件会成功传输；只有生成产物可能失败，而这些文件无论如何都会由开发服务器重新生成。

## 第 2 层:mutagen（持续同步）

在初始 rsync 之后，守护进程会在本地 shell 容器内启动一个 mutagen 会话:

```bash
docker exec {shell_container} mutagen sync create \
    --name coast-{project}-{instance} \
    --sync-mode one-way-safe \
    --ignore-vcs \
    --ignore node_modules --ignore target \
    --ignore __pycache__ --ignore .next \
    /workspace/ {user}@{host}:{remote_workspace}/
```

Mutagen 通过操作系统级事件监视文件变化（容器内的 inotify），对变更进行批处理，并通过持久 SSH 连接传输增量。你的编辑会在几秒内出现在远端。

### one-way-safe 模式

Mutagen 以 `one-way-safe` 模式运行:更改只会从本地流向远端。远端创建的文件（由开发服务器、构建工具等生成）不会同步回你的本地机器。这可以防止生成产物污染你的工作目录。

### Mutagen 是运行时依赖

Mutagen 安装在:

- **coast 镜像** 中（由 `coast build` 根据 `[coast.setup]` 构建），供本地 shell 容器使用。
- **coast-service Docker 镜像**（`Dockerfile.coast-service`）中，供远端使用。

守护进程绝不会直接在主机上运行 mutagen。它通过 `docker exec` 进入 shell 容器进行编排。

## 生命周期

| Command | rsync | mutagen |
|---------|-------|---------|
| `coast run` | 初始完整传输 | rsync 后创建会话 |
| `coast assign` | 新 worktree 的增量传输 | 旧会话终止，创建新会话 |
| `coast stop` | -- | 会话终止 |
| `coast rm` | -- | 会话终止 |

### 回退行为

如果 mutagen 会话无法在 shell 容器内启动，守护进程会记录一条警告。初始 rsync 仍然会提供工作区内容，但文件更改不会实时同步，直到会话重新建立（例如在下一次 `coast assign` 或守护进程重启时）。

## 同步策略配置

Coastfile 中的 `[remote]` 部分控制同步策略:

```toml
[remote]
workspace_sync = "mutagen"    # "rsync" (default) or "mutagen"
```

- **`rsync`**（默认）:只执行初始 rsync 传输。不进行持续同步。适用于不需要实时同步的 CI 环境或批处理作业。
- **`mutagen`**:先使用 rsync 进行初始传输，然后使用 mutagen 进行持续同步。适用于交互式开发，在这种场景下你希望编辑内容能立即出现在远端。

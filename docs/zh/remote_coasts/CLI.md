# CLI 和配置

本页介绍 `coast remote` 命令组、`Coastfile.remote` 配置格式，以及远程机器的磁盘管理。

## 远程管理命令

### `coast remote add`

向守护进程注册一台远程机器:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

示例:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

连接详细信息存储在守护进程的 `state.db` 中。它们绝不会存储在 Coastfile 中。

### `coast remote ls`

列出所有已注册的远程机器:

```bash
coast remote ls
```

### `coast remote rm`

删除一个已注册的远程机器:

```bash
coast remote rm <name>
```

如果远程机器上仍有实例在运行，请先使用 `coast rm` 将其删除。

### `coast remote test`

验证 SSH 连通性和 coast-service 可用性:

```bash
coast remote test <name>
```

此命令会检查 SSH 访问，确认可通过 SSH 隧道在 31420 端口访问 coast-service，并报告远程机器的架构和 coast-service 版本。

### `coast remote prune`

清理远程机器上的孤立资源:

```bash
coast remote prune <name>              # remove orphaned resources
coast remote prune <name> --dry-run    # preview what would be removed
```

Prune 会通过将 Docker 卷和工作区目录与 coast-service 实例数据库进行交叉比对来识别孤立资源。属于活动实例的资源绝不会被删除。

## Coastfile 配置

远程 coast 使用一个单独的 Coastfile，它会扩展你的基础配置。文件名决定类型:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### 最小示例

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### `[remote]` 部分

`[remote]` 部分声明同步偏好。连接详细信息（host、user、SSH key）来自 `coast remote add`，并在运行时解析。

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | 同步策略:`"rsync"` 仅用于一次性批量传输，`"mutagen"` 用于 rsync + 持续实时同步 |

### 验证约束

1. 当 Coastfile 类型以 `remote` 开头时，必须包含 `[remote]` 部分。
2. 非远程 Coastfile 不能包含 `[remote]` 部分。
3. 不支持内联主机配置。连接详细信息必须来自已注册的远程机器。
4. 使用 `strategy = "shared"` 的共享卷会在远程主机上创建一个 Docker 卷，并由该远程主机上的所有 coast 共享。该卷不会在不同远程机器之间分发。

### 继承

远程 Coastfile 与其他带类型的 Coastfile 一样，使用相同的[继承系统](../coastfiles/INHERITANCE.md)。`extends = "Coastfile"` 指令会将基础配置与远程覆盖项合并。你可以像处理其他带类型变体一样覆盖端口、服务、卷并分配策略。

## 磁盘管理

### 每实例资源使用量

每个远程 coast 实例大约会消耗:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

对于具有 2-3 个并发实例的典型项目，建议最小磁盘空间为:**50 GB**。

### 资源命名约定

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### `coast rm` 时的清理

当 `coast rm` 删除一个远程实例时，它会清理:

1. 远程 DinD 容器（通过 coast-service）
2. DinD Docker 卷（`coast-dind--{project}--{name}`）
3. 工作区目录（`/data/workspaces/{project}/{name}`）
4. 本地影子实例记录、端口分配和 shell 容器

### 何时执行 prune

如果在删除实例后，远程机器上的 `df -h` 仍显示磁盘使用率很高，可能存在失败或中断操作遗留下来的孤立资源。运行 `coast remote prune` 以回收空间:

```bash
# See what would be removed
coast remote prune my-vm --dry-run

# Actually remove
coast remote prune my-vm
```

Prune 绝不会删除属于活动实例的资源。

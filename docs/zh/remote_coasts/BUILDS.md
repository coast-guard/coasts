# 远程构建

远程构建通过 coast-service 在远程机器上运行。这可确保构建使用远程机器的原生架构（例如 EC2 实例上的 `x86_64`），而不受你本地架构（例如 ARM Mac）的影响。无需交叉编译或架构模拟。

## 工作原理

当你运行 `coast build --type remote` 时，会发生以下过程:

1. 守护进程通过 SSH 使用 rsync 将项目源文件（Coastfile、compose.yml、Dockerfiles、inject/）同步到远程工作区。
2. 守护进程通过 SSH 隧道向 coast-service 调用 `POST /build`。
3. coast-service 在远程机器上以原生方式运行完整构建:`docker build`、镜像拉取、镜像缓存和密钥提取，全部在 `/data/images/` 下执行。
4. coast-service 返回一个 `BuildResponse`，其中包含产物路径和构建元数据。
5. 守护进程使用 rsync 将完整的产物目录（coastfile.toml、compose.yml、manifest.json、secrets/、inject/、镜像 tarballs）同步回你本地机器上的 `~/.coast/images/{project}/{build_id}/`。
6. 守护进程创建一个指向新构建的 `latest-remote` 符号链接。

```text
Local Machine                              Remote Machine
┌─────────────────────────────┐            ┌───────────────────────────┐
│  ~/.coast/images/my-app/    │            │  /data/images/my-app/     │
│    latest-remote -> {id}    │  ◀─rsync─  │    {id}/                  │
│    {id}/                    │            │      manifest.json        │
│      manifest.json          │            │      coastfile.toml       │
│      coastfile.toml         │            │      compose.yml          │
│      compose.yml            │            │      *.tar (images)       │
│      *.tar (images)         │            │                           │
└─────────────────────────────┘            └───────────────────────────┘
```

## 命令

```bash
# Build on the default remote (auto-selected if only one registered)
coast build --type remote

# Build on a specific remote
coast build --type remote --remote my-vm

# Build without running (standalone)
coast build --type remote
```

如果尚不存在兼容的构建，`coast run --type remote` 也会触发一次构建。

## 架构匹配

每个构建的 `manifest.json` 都会记录其构建目标架构（例如 `aarch64`、`x86_64`）。当你运行 `coast run --type remote` 时，守护进程会检查现有构建是否与目标远程机器的架构匹配:

- **架构匹配**:复用该构建。无需重新构建。
- **架构不匹配**:守护进程会搜索具有正确架构的最新构建。如果不存在，则会返回错误并给出重新构建的指引。

这意味着你可以在一个 x86_64 远程机器上构建一次，然后部署到任意数量的 x86_64 远程机器，而无需重新构建。但你不能在 x86_64 远程机器上使用 ARM 构建，反之亦然。

## 符号链接

远程构建使用与本地构建分开的符号链接:

| Symlink | Points to |
|---------|-----------|
| `latest` | 最新的本地构建 |
| `latest-remote` | 最新的远程构建 |
| `latest-{type}` | 特定 Coastfile 类型的最新本地构建 |

这种分离可防止远程构建覆盖你的本地 `latest` 符号链接，反之亦然。

## 自动清理

对于每个 `(coastfile_type, architecture)` 组合，Coast 最多保留 5 个远程构建。每次远程构建成功后，超出限制的旧构建都会被自动删除。

正在被运行中的实例使用的构建永远不会被清理，无论是否超过限制。如果你有 7 个 x86_64 远程构建，但其中 3 个正在为活跃实例提供支持，那么这 3 个都会受到保护。

清理是按架构感知的:如果你同时拥有 `aarch64` 和 `x86_64` 的远程构建，则每种架构都会各自独立维护自己的 5 个构建池。

## 产物存储

远程构建产物会存储在两个位置:

| Location | Path | Purpose |
|----------|------|---------|
| Remote | `/data/images/{project}/{build_id}/` | 远程机器上的事实来源 |
| Local | `~/.coast/images/{project}/{build_id}/` | 用于跨远程机器复用的本地缓存 |

远程机器上的 `/data/image-cache/` 镜像缓存会在所有项目之间共享，就像本地的 `~/.coast/image-cache/` 一样。

## 与本地构建的关系

远程构建和本地构建彼此独立。`coast build`（不带 `--type remote`）始终在你的本地机器上构建，并更新 `latest` 符号链接。`coast build --type remote` 始终在远程机器上构建，并更新 `latest-remote` 符号链接。

你可以让同一个项目的本地构建和远程构建同时共存。本地 coast 使用本地构建；远程 coast 使用远程构建。

有关构建工作方式的一般说明（manifest 结构、镜像缓存、类型化构建），请参阅 [Builds](../concepts_and_terminology/BUILDS.md)。

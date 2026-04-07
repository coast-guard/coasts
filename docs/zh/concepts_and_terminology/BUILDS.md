# 构建

可以把 coast 构建理解为一个带有额外辅助能力的 Docker 镜像。构建是一个基于目录的工件，打包了创建 Coast 实例所需的一切:已解析的 [Coastfile](COASTFILE_TYPES.md)、重写后的 compose 文件、预先拉取的 OCI 镜像 tarball，以及注入的主机文件。它本身不是 Docker 镜像，但它包含 Docker 镜像（以 tarball 形式）以及 Coast 用来将它们连接在一起所需的元数据。

## `coast build` 的作用

当你运行 `coast build` 时，守护进程会按顺序执行以下步骤:

1. 解析并验证 Coastfile。
2. 读取 compose 文件并过滤掉被省略的服务。
3. 从已配置的提取器中提取 [secrets](SECRETS.md)，并将其加密存储到 keystore 中。
4. 为具有 `build:` 指令的 compose 服务在主机上构建 Docker 镜像。
5. 为具有 `image:` 指令的 compose 服务拉取 Docker 镜像。
6. 将所有镜像作为 OCI tarball 缓存到 `~/.coast/image-cache/` 中。
7. 如果配置了 `[coast.setup]`，则使用指定的软件包、命令和文件构建自定义 DinD 基础镜像。
8. 写入构建工件目录，其中包含 manifest、已解析的 coastfile、重写后的 compose 文件和注入的文件。
9. 更新 `latest` 符号链接，使其指向新的构建。
10. 自动清理超出保留数量限制的旧构建。

## 构建存储位置

```text
~/.coast/
  images/
    my-project/
      latest -> a3c7d783_20260227143000       (symlink)
      a3c7d783_20260227143000/                (versioned build)
        manifest.json
        coastfile.toml
        compose.yml
        inject/
      b4d8e894_20260226120000/                (older build)
        ...
  image-cache/                                (shared tarball cache)
    postgres_16_a1b2c3d4e5f6.tar
    redis_7_f6e5d4c3b2a1.tar
    coast-built_my-project_web_latest_...tar
```

每个构建都会获得一个唯一的 **build ID**，格式为 `{coastfile_hash}_{YYYYMMDDHHMMSS}`。该哈希包含 Coastfile 内容和已解析配置，因此对 Coastfile 的更改会生成新的 build ID。

`latest` 符号链接始终指向最近的构建，以便快速解析。如果你的项目使用类型化 Coastfile（例如 `Coastfile.light`），则每种类型都有自己的符号链接:`latest-light`。

位于 `~/.coast/image-cache/` 的镜像缓存由所有项目共享。如果两个项目使用相同的 Postgres 镜像，则该 tarball 只会缓存一次。

## 构建包含的内容

每个构建目录包含:

- **`manifest.json`** -- 完整的构建元数据:项目名称、构建时间戳、coastfile 哈希、已缓存/已构建镜像列表、secret 名称、被省略的服务、[卷策略](VOLUMES.md) 等。
- **`coastfile.toml`** -- 已解析的 Coastfile（如果使用 `extends`，则会与父配置合并）。
- **`compose.yml`** -- 你的 compose 文件的重写版本，其中 `build:` 指令会被替换为预构建镜像标签，并移除被省略的服务。
- **`inject/`** -- 来自 `[inject].files` 的主机文件副本（例如 `~/.gitconfig`、`~/.npmrc`）。

## 构建不包含 Secrets

Secrets 会在构建步骤期间被提取，但它们会存储在单独的加密 keystore `~/.coast/keystore.db` 中——而不是构建工件目录中。manifest 只记录被提取 secret 的**名称**，绝不记录其值。

这意味着构建工件可以安全地被检查，而不会暴露敏感数据。Secrets 会在之后、通过 `coast run` 创建 Coast 实例时被解密并注入。

## 构建与 Docker

一个构建涉及三类 Docker 镜像:

- **已构建镜像** -- 带有 `build:` 指令的 compose 服务会通过 `docker build` 在主机上构建，标记为 `coast-built/{project}/{service}:latest`，并作为 tarball 保存到镜像缓存中。
- **已拉取镜像** -- 带有 `image:` 指令的 compose 服务会被拉取并保存为 tarball。
- **Coast 镜像** -- 如果配置了 `[coast.setup]`，则会基于 `docker:dind` 构建一个自定义 Docker 镜像，并包含指定的软件包、命令和文件。标记为 `coast-image/{project}:{build_id}`。

在运行时（[`coast run`](RUN.md)），这些 tarball 会通过 `docker load` 加载到内部的 [DinD 守护进程](RUNTIMES_AND_SERVICES.md)中。这也是 Coast 实例能够快速启动、无需从镜像仓库拉取镜像的原因。

## 构建与实例

当你运行 [`coast run`](RUN.md) 时，Coast 会解析最新构建（或特定的 `--build-id`），并使用其工件来创建实例。实例上会记录该 build ID。

你不需要为了创建更多实例而重新构建。一个构建可以服务于多个并行运行的 Coast 实例。

## 何时重新构建

只有当你的 Coastfile、`docker-compose.yml` 或基础设施配置发生变化时，才需要重新构建。重新构建是资源密集型操作——它会重新拉取镜像、重新构建 Docker 镜像，并重新提取 secrets。

代码变更不需要重新构建。Coast 会将你的项目目录直接挂载到每个实例中，因此代码更新会被立即获取。

## 自动清理

Coast 对每种 Coastfile 类型最多保留 5 个构建。每次 `coast build` 成功后，超出限制的旧构建都会被自动删除。

正在被运行中实例使用的构建永远不会被清理，无论限制是多少。如果你有 7 个构建，但其中 3 个正在为活跃实例提供支持，那么这 3 个都会受到保护。

## 手动删除

你可以通过 `coast rm-build` 或 Coastguard 的 Builds 标签页手动删除构建。

- **完整项目删除**（`coast rm-build <project>`）要求先停止并删除所有实例。它会删除整个构建目录、关联的 Docker 镜像、卷和容器。
- **选择性删除**（按 build ID，在 Coastguard UI 中可用）会跳过正在被运行中实例使用的构建。

## 类型化构建

如果你的项目使用多个 Coastfile（例如，默认配置使用 `Coastfile`，而快照预置卷使用 `Coastfile.snap`），则每种类型都会维护自己的 `latest-{type}` 符号链接，以及各自的 5 个构建清理池。

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

清理 `snap` 构建永远不会影响 `default` 构建，反之亦然。

## 远程构建

在为 [remote coast](REMOTES.md) 构建时，构建会通过 `coast-service` 在远程机器上运行，以便镜像使用远程机器的原生架构。随后，该工件会被传回你的本地机器以供复用。远程构建维护它们自己的 `latest-remote` 符号链接，并按架构进行清理。详见 [Remotes](REMOTES.md)。

# 构建（Builds）

把一次 Coast 构建想象成一个带有额外辅助功能的 Docker 镜像。构建是一个基于目录的产物（artifact），打包了创建 Coast 实例所需的一切:已解析的 [Coastfile](COASTFILE_TYPES.md)、重写后的 compose 文件、预拉取的 OCI 镜像 tar 包，以及注入的主机文件。它本身不是一个 Docker 镜像，但它包含 Docker 镜像（以 tar 包形式）以及 Coast 用来将它们连接起来所需的元数据。

## `coast build` 做了什么

当你运行 `coast build` 时，守护进程会按顺序执行以下步骤:

1. 解析并校验 Coastfile。
2. 读取 compose 文件并过滤掉被省略的服务。
3. 从已配置的提取器中提取 [secrets](SECRETS.md) 并将其加密存储到密钥库中。
4. 为具有 `build:` 指令的 compose 服务（在主机上）构建 Docker 镜像。
5. 为具有 `image:` 指令的 compose 服务拉取 Docker 镜像。
6. 将所有镜像以 OCI tar 包形式缓存到 `~/.coast/image-cache/`。
7. 如果配置了 `[coast.setup]`，则使用指定的包、命令和文件构建一个自定义 DinD 基础镜像。
8. 写入构建产物目录，其中包含清单（manifest）、已解析的 coastfile、重写后的 compose，以及被注入的文件。
9. 更新 `latest` 符号链接以指向新的构建。
10. 自动清理超出保留上限的旧构建。

## 构建存放在哪里

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

每个构建都会获得一个唯一的 **构建 ID**，格式为 `{coastfile_hash}_{YYYYMMDDHHMMSS}`。该哈希会包含 Coastfile 内容和已解析配置，因此对 Coastfile 的更改会生成新的构建 ID。

`latest` 符号链接始终指向该项目最新的构建，以便快速解析。如果你的项目使用带类型的 Coastfile（例如 `Coastfile.light`），每种类型都有自己的符号链接:`latest-light`。

位于 `~/.coast/image-cache/` 的镜像缓存会在所有项目之间共享。如果两个项目使用相同的 Postgres 镜像，tar 包只会缓存一次。

## 一个构建包含什么

每个构建目录包含:

- **`manifest.json`** -- 完整的构建元数据:项目名称、构建时间戳、coastfile 哈希、已缓存/已构建镜像列表、secret 名称、被省略的服务、[卷策略](VOLUMES.md) 等等。
- **`coastfile.toml`** -- 已解析的 Coastfile（如果使用 `extends`，则与父级合并）。
- **`compose.yml`** -- 你 compose 文件的重写版本，其中 `build:` 指令会被替换为预构建镜像标签，并移除被省略的服务。
- **`inject/`** -- 来自 `[inject].files` 的主机文件副本（例如 `~/.gitconfig`、`~/.npmrc`）。

## 构建不包含 Secrets

Secrets 会在构建步骤中被提取，但它们会存储在单独的加密密钥库 `~/.coast/keystore.db` 中——而不是在构建产物目录里。清单只会记录被提取的 secret **名称**，绝不会记录其值。

这意味着构建产物可以安全地被查看而不会暴露敏感数据。Secrets 会在之后创建 Coast 实例（`coast run`）时才被解密并注入。

## 构建与 Docker

一次构建涉及三类 Docker 镜像:

- **已构建镜像** -- 带有 `build:` 指令的 compose 服务会在主机上通过 `docker build` 构建，标记为 `coast-built/{project}/{service}:latest`，并以 tar 包形式保存到镜像缓存中。
- **已拉取镜像** -- 带有 `image:` 指令的 compose 服务会被拉取并保存为 tar 包。
- **Coast 镜像** -- 如果配置了 `[coast.setup]`，则会在 `docker:dind` 之上构建一个自定义 Docker 镜像，并包含指定的包、命令和文件。标记为 `coast-image/{project}:{build_id}`。

在运行时（`coast run`），这些 tar 包会通过 `docker load` 被加载到内部的 [DinD 守护进程](RUNTIMES_AND_SERVICES.md) 中。这使得 Coast 实例能够快速启动，而无需从镜像仓库拉取镜像。

## 构建与实例

当你运行 `coast run` 时，Coast 会解析最新构建（或某个指定的 `--build-id`），并使用其产物来创建实例。构建 ID 会记录在实例上。

你不需要重新构建来创建更多实例。一次构建可以服务于多个并行运行的 Coast 实例。

## 何时需要重新构建

只有当你的 Coastfile、`docker-compose.yml` 或基础设施配置发生变化时才需要重新构建。重新构建会消耗大量资源——它会重新拉取镜像、重新构建 Docker 镜像，并重新提取 secrets。

代码变更不需要重新构建。Coast 会将你的项目目录直接挂载进每个实例，因此代码更新会被立即拾取。

## 自动清理（Auto-Pruning）

Coast 对每种 Coastfile 类型最多保留 5 个构建。每次成功执行 `coast build` 之后，超过上限的旧构建会被自动移除。

正在被运行中实例使用的构建永远不会被清理，无论上限是多少。如果你有 7 个构建，但其中 3 个正在支撑活跃实例，那么这 3 个都会受到保护。

## 手动移除

你可以通过 `coast rm-build` 或通过 Coastguard 的 Builds 选项卡手动移除构建。

- **移除整个项目**（`coast rm-build <project>`）要求先停止并移除所有实例。它会移除整个构建目录、关联的 Docker 镜像、卷和容器。
- **选择性移除**（按构建 ID，在 Coastguard UI 中可用）会跳过正在被运行中实例使用的构建。

## 带类型的构建（Typed Builds）

如果你的项目使用多个 Coastfile（例如 `Coastfile` 用于默认配置，`Coastfile.snap` 用于快照播种卷），每种类型都会维护自己的 `latest-{type}` 符号链接以及各自独立的 5 个构建清理池。

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

清理 `snap` 构建永远不会影响 `default` 构建，反之亦然。

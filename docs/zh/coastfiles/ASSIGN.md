# Assign

`[assign]` 部分用于控制在你使用 `coast assign` 切换分支时，Coast 实例内的服务会发生什么。每个服务都可以根据其需求配置不同的策略:是否需要完整重建、重启、热重载，或者什么都不做。

关于 `coast assign` 和 `coast unassign` 在运行时如何工作，请参阅 [Assign](../concepts_and_terminology/ASSIGN.md)。

## `[assign]`

### `default`

在分支切换时应用于所有服务的默认动作。如果整个 `[assign]` 部分被省略，则默认值为 `"restart"`。

- **`"none"`** — 什么都不做。服务保持按原样运行。适用于不依赖代码的数据库和缓存。
- **`"hot"`** — 代码已通过 [filesystem](../concepts_and_terminology/FILESYSTEM.md) 进行实时挂载，因此服务会自动获取变更（例如通过文件监听器或热重载）。无需重启容器。
- **`"restart"`** — 重启服务容器。用于服务在启动时读取代码，但不需要完整镜像重建的场景。
- **`"rebuild"`** — 重建服务的 Docker 镜像并重启。当代码通过 Dockerfile 中的 `COPY` 或 `ADD` 被打包进镜像时必需。

```toml
[assign]
default = "none"
```

### `[assign.services]`

按服务覆盖。每个键是一个 compose 服务名，值为上述四种动作之一。

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

这让你可以让数据库和缓存保持不变（通过默认值 `"none"`），同时只对依赖发生变更的代码的服务进行重建或重启。

### `[assign.rebuild_triggers]`

用于为特定服务强制触发重建的文件模式，即使它们的默认动作是更轻量的操作。每个键是一个服务名，值是文件路径或模式的列表。

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

在执行 `coast assign` 期间，从 worktree 同步中排除的路径列表。适用于大型 monorepo，其中某些目录与 Coast 中运行的服务无关，否则会拖慢 assign 操作。

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Examples

### 重建 app，其余保持不变

当你的 app 服务将代码打包进其 Docker 镜像，但数据库不受代码变更影响时:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### 前端和后端热重载

当两个服务都使用文件监听器（例如 Next.js dev server、Go air、nodemon）且代码为实时挂载时:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### 带触发器的按服务重建

API 服务通常只重启，但如果 `Dockerfile` 或 `package.json` 发生变更，则会重建:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### 全部服务完全重建

当所有服务都将代码打包进其镜像时:

```toml
[assign]
default = "rebuild"
```

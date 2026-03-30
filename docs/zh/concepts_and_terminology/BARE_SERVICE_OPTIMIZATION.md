# 裸服务优化

[裸服务](BARE_SERVICES.md) 作为普通进程在 Coast 容器内运行。由于没有 Docker 层或镜像缓存，启动和分支切换性能取决于你如何组织 `install` 命令、缓存和 assign 策略。

## 快速安装命令

`install` 字段会在服务启动前运行，并且在每次 `coast assign` 时再次运行。如果 `install` 无条件执行 `make` 或 `yarn install`，那么每次分支切换都要承担完整的安装成本，即使什么都没有变化。

**尽可能使用条件检查来跳过不必要的工作:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

如果 `node_modules` 已经存在，`test -f` 保护条件会跳过安装。首次运行或缓存未命中后，它会执行完整安装。在后续依赖未发生变化的 assign 中，它会立即完成。

对于已编译的二进制文件，检查输出是否存在:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## 在 worktree 之间缓存目录

当 Coast 将一个裸服务实例切换到新的 worktree 时，`/workspace` 挂载会切换到不同的目录。像 `node_modules` 或已编译二进制文件这样的构建产物会留在旧的 worktree 中。`cache` 字段告诉 Coast 在切换之间保留指定目录:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

缓存目录会在 worktree 重新挂载之前备份，并在之后恢复。这意味着 `yarn install` 会以增量方式运行，而不是从头开始，已编译的二进制文件也能在分支切换后保留下来。

## 使用 private_paths 隔离每个实例的目录

有些工具会在工作区中创建包含每个进程状态的目录:锁文件、构建缓存或 PID 文件。当多个 Coast 实例共享同一个工作区时（同一分支、没有 worktree），这些目录会发生冲突。

经典示例是 Next.js，它在启动时会对 `.next/dev/lock` 加锁。第二个 Coast 实例会看到该锁并拒绝启动。

`private_paths` 为指定路径上的每个实例提供其各自独立隔离的目录:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

每个实例都会在该路径上获得一个按实例划分的 overlay 挂载。锁文件、构建缓存和 Turbopack 状态都会被完全隔离。不需要修改代码。

对于任何并发实例写入同一文件会引发问题的目录，都应使用 `private_paths`:`.next`、`.turbo`、`.parcel-cache`、PID 文件或 SQLite 数据库。

## 连接到共享服务

当你将[共享服务](SHARED_SERVICES.md)用于数据库或缓存时，这些共享容器运行在主机 Docker 守护进程上，而不是在 Coast 内部。在 Coast 内部运行的裸服务无法通过 `localhost` 访问它们。

请改用 `host.docker.internal`:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

你也可以使用[密钥](../coastfiles/SECRETS.md)将连接字符串作为环境变量注入:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Coast 内部的 compose 服务不存在这个问题。Coast 会自动通过桥接网络为 compose 容器路由共享服务主机名。这只影响裸服务。

## 内联环境变量

裸服务命令会继承来自 Coast 容器的环境变量，包括通过 `.env` 文件、密钥和 inject 设置的任何内容。但有时你需要为单个服务覆盖某个特定变量，而不修改共享配置文件。

在命令前加上内联赋值即可:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

内联变量优先级高于其他所有来源。这适用于:

- 将 `AUTH_URL` 设置为[动态端口](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md)，以便认证重定向在未 checkout 的实例上正常工作
- 覆盖 `DATABASE_URL`，使其通过 `host.docker.internal` 指向共享服务
- 在不修改工作区中的共享 `.env` 文件的情况下设置特定于服务的标志

## 裸服务的 Assign 策略

根据每个服务如何获取代码变更来选择合适的 [assign 策略](../coastfiles/ASSIGN.md):

| Strategy | 何时使用 | 示例 |
|---|---|---|
| `hot` | 服务具有文件监视器，在 worktree 重新挂载后能够自动检测更改 | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | 服务在启动时加载代码，并且不会监视更改 | 已编译的 Go 二进制文件, Rails, Java 服务器 |
| `none` | 服务不依赖工作区代码，或使用单独的索引 | 数据库服务器, Redis, 搜索索引 |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

将默认值设置为 `none` 意味着基础设施服务在分支切换时永远不会被触及。只有关心代码变更的服务才会被重启或依赖热重载。

## 另请参阅

- [裸服务](BARE_SERVICES.md) - 完整的裸服务参考
- [性能优化](PERFORMANCE_OPTIMIZATIONS.md) - 一般性能调优，包括 `exclude_paths` 和 `rebuild_triggers`
- [动态端口环境变量](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 在命令中使用 `WEB_DYNAMIC_PORT` 及相关变量

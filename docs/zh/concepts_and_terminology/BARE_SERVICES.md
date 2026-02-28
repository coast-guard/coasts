# 裸服务

如果你可以把项目容器化，你就应该这么做。裸服务是为那些尚未容器化、并且在短期内添加 `Dockerfile` 和 `docker-compose.yml` 并不现实的项目而存在的。它们是一个过渡台阶，而不是终点。

与使用 `docker-compose.yml` 来编排容器化服务不同，裸服务允许你在 Coastfile 中定义 shell 命令，Coast 会在 Coast 容器内通过一个轻量级的监督器将它们作为普通进程运行。

## 为什么应该选择容器化

[Docker Compose](RUNTIMES_AND_SERVICES.md) 服务为你提供:

- 通过 Dockerfile 实现可复现的构建
- Coast 可以在启动时等待的健康检查
- 服务之间的进程隔离
- 由 Docker 处理的卷与网络管理
- 可移植的定义，可在 CI、预发布环境与生产环境中工作

裸服务不提供上述任何能力。你的进程共享同一个文件系统，崩溃恢复只是一个 shell 循环，而“在我机器上能跑”在 Coast 内部和外部一样可能发生。如果你的项目已经有 `docker-compose.yml`，就使用它。

## 什么时候裸服务有意义

- 你正在为一个从未容器化的项目引入 Coast，并且希望立即从 worktree 隔离与端口管理中获得价值
- 你的项目是单进程工具或 CLI，写 Dockerfile 反而是大材小用
- 你想逐步迭代容器化——先用裸服务，之后再迁移到 compose

## 配置

裸服务通过 Coastfile 中的 `[services.<name>]` 段落来定义。一个 Coastfile **不能** 同时定义 `compose` 和 `[services]`——它们是互斥的。

```toml
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

每个服务有四个字段:

| 字段 | 必需 | 描述 |
|---|---|---|
| `command` | 是 | 要运行的 shell 命令（例如 `"npm run dev"`） |
| `port` | 否 | 服务监听的端口，用于端口映射 |
| `restart` | 否 | 重启策略:`"no"`（默认）、`"on-failure"` 或 `"always"` |
| `install` | 否 | 启动前运行的一条或多条命令（例如 `"npm install"` 或 `["npm install", "npm run build"]`） |

### Setup Packages

由于裸服务以普通进程运行，Coast 容器需要安装正确的运行时。使用 `[coast.setup]` 来声明系统包:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

这些会在任何服务启动前安装。否则，你的 `npm` 或 `node` 命令会在容器内失败。

### Install Commands

`install` 字段会在服务启动前运行，并且在每次 [`coast assign`](ASSIGN.md)（切换分支）时再次运行。这是放置依赖安装的地方:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

安装命令按顺序执行。如果任何一条安装命令失败，服务将不会启动。

### Restart Policies

- **`no`** — 服务只运行一次。如果退出，就保持停止。用于一次性任务或你希望手动管理的服务。
- **`on-failure`** — 当服务以非零退出码退出时重启。成功退出（退出码 0）则不处理。使用从 1 秒到 30 秒的指数退避，并在连续崩溃 10 次后放弃。
- **`always`** — 任意退出都重启，包括成功退出。与 `on-failure` 相同的退避策略。用于不应停止的长期运行服务器。

如果服务在崩溃前运行超过 30 秒，则重试计数与退避会重置——假设它曾在一段时间内是健康的，而这次崩溃是一个新问题。

## 底层工作原理

```text
┌─── Coast: dev-1 ──────────────────────────────────────┐
│                                                       │
│   /coast-supervisor/                                  │
│   ├── web.sh          (runs command, tracks PID)      │
│   ├── worker.sh                                       │
│   ├── start-all.sh    (launches all services)         │
│   ├── stop-all.sh     (SIGTERM via PID files)         │
│   └── ps.sh           (checks PID liveness)           │
│                                                       │
│   /var/log/coast-services/                            │
│   ├── web.log                                         │
│   └── worker.log                                      │
│                                                       │
│   No inner Docker daemon images are used.             │
│   Processes run directly on the container OS.         │
└───────────────────────────────────────────────────────┘
```

Coast 会为每个服务生成 shell 脚本包装器，并将其放置在 DinD 容器内的 `/coast-supervisor/` 中。每个包装器会跟踪其 PID，将输出重定向到日志文件，并通过 shell 循环实现重启策略。这里没有 Docker Compose、没有内部 Docker 镜像，也没有服务之间的容器级隔离。

`coast ps` 通过检查 PID 是否存活来工作，而不是查询 Docker；`coast logs` 通过 tail 日志文件来工作，而不是调用 `docker compose logs`。日志输出格式与 compose 的 `service | line` 格式一致，因此 Coastguard 的 UI 无需改动即可工作。

## 端口

端口配置与基于 compose 的 Coast 完全相同。在 `[ports]` 中定义你的服务监听的端口:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[动态端口](PORTS.md) 会在 `coast run` 时分配，而 [`coast checkout`](CHECKOUT.md) 会像往常一样交换规范端口。唯一的区别是服务之间没有 Docker 网络——它们都直接绑定到容器的 loopback 或 `0.0.0.0`。

## 分支切换

当你在一个裸服务的 Coast 上运行 `coast assign` 时，会发生以下事情:

1. 通过 SIGTERM 停止所有正在运行的服务
2. worktree 切换到新分支
3. 重新运行安装命令（例如 `npm install` 会获取新分支的依赖）
4. 重启所有服务

这等同于使用 compose 时发生的过程——`docker compose down`、切换分支、重建、`docker compose up`——只是这里用的是 shell 进程而不是容器。

## 限制

- **没有健康检查。** Coast 无法像对定义了健康检查的 compose 服务那样等待裸服务变为“健康”。它只会启动进程并寄希望于一切正常。
- **服务之间没有隔离。** 所有进程在 Coast 容器内共享同一个文件系统与进程命名空间。行为异常的服务可能影响其他服务。
- **没有构建缓存。** Docker Compose 构建会逐层缓存。裸服务的 `install` 命令在每次 assign 时都会从头运行。
- **崩溃恢复很基础。** 重启策略使用带指数退避的 shell 循环。它不是 systemd 或 supervisord 这样的进程监督器。
- **服务不支持 `[omit]` 或 `[unset]`。** Coastfile 的类型组合适用于 compose 服务，但裸服务不支持通过类型化 Coastfile 来省略单个服务。

## 迁移到 Compose

当你准备好容器化时，迁移路径很直接:

1. 为每个服务编写一个 `Dockerfile`
2. 创建一个引用它们的 `docker-compose.yml`
3. 用指向你的 compose 文件的 `compose` 字段替换 Coastfile 中的 `[services.*]` 段落
4. 移除那些现在由 Dockerfile 处理的 `[coast.setup]` 包
5. 使用 [`coast build`](BUILDS.md) 重新构建

你的端口映射、[卷](VOLUMES.md)、[共享服务](SHARED_SERVICES.md) 与 [secrets](SECRETS.md) 配置都会原样沿用不变。唯一变化的是服务本身的运行方式。

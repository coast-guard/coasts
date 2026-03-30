# 裸服务

如果你能把项目容器化，你就应该这么做。裸服务适用于尚未容器化的项目，以及在短期内添加 `Dockerfile` 和 `docker-compose.yml` 不现实的情况。

裸服务不是用 `docker-compose.yml` 来编排容器化服务，而是让你在 Coastfile 中定义 shell 命令，Coast 会在 Coast 容器内使用一个轻量级监督器将它们作为普通进程运行。

## 为什么应该改用容器化

[Docker Compose](RUNTIMES_AND_SERVICES.md) 服务为你提供:

- 通过 Dockerfile 实现可复现的构建
- Coast 在启动期间可以等待的健康检查
- 服务之间的进程隔离
- 由 Docker 处理卷和网络管理
- 可移植的定义，可在 CI、预发布和生产环境中工作

裸服务不提供上述任何能力。你的进程共享同一个文件系统，崩溃恢复只是一个 shell 循环，而“在我机器上能跑”在 Coast 里同样可能发生（和在 Coast 外一样）。如果你的项目已经有 `docker-compose.yml`，就用它。

## 何时裸服务有意义

- 你正在为一个从未容器化的项目引入 Coast，并希望立刻从工作树隔离和端口管理中获得价值
- 你的项目是一个单进程工具或 CLI，写 Dockerfile 显得大材小用
- 你想逐步推进容器化，从裸服务开始，之后再迁移到 compose

## 配置

裸服务在 Coastfile 中通过 `[services.<name>]` 段落定义。一个 Coastfile 可以只定义裸服务，也可以与 `compose` 并存——后者请参阅 [Mixed Service Types](MIXED_SERVICE_TYPES.md)。

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

| Field | Required | Description |
|---|---|---|
| `command` | yes | 要运行的 shell 命令（例如 `"npm run dev"`） |
| `port` | no | 服务监听的端口，用于端口映射 |
| `restart` | no | 重启策略:`"no"`（默认）、`"on-failure"` 或 `"always"` |
| `install` | no | 启动前要运行的一条或多条命令（例如 `"npm install"` 或 `["npm install", "npm run build"]`） |

### Setup Packages

由于裸服务以普通进程方式运行，Coast 容器需要安装正确的运行时。使用 `[coast.setup]` 来声明系统包:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

这些会在任何服务启动之前安装。否则，你的 `npm` 或 `node` 命令会在容器内失败。

### Install Commands

`install` 字段会在服务启动前运行，并且在每次 [`coast assign`](ASSIGN.md)（切换分支）时再次运行。这是放置依赖安装的位置:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

安装命令会按顺序执行。只要有任意一条安装命令失败，服务就不会启动。

### Restart Policies

- **`no`**: 服务只运行一次。若退出，将保持停止状态。用于一次性任务或你想手动管理的服务。
- **`on-failure`**: 当服务以非零退出码退出时重启。正常退出（退出码 0）不会重启。使用从 1 秒到 30 秒的指数退避，并在连续崩溃 10 次后放弃。
- **`always`**: 任何退出都会重启，包括正常退出。与 `on-failure` 相同的退避策略。用于不应停止的长时间运行服务器。

如果服务在崩溃前运行超过 30 秒，则重试计数与退避会重置——假设它曾经健康运行了一段时间，此次崩溃是一个新问题。

## How It Works

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

Coast 会为每个服务生成 shell 脚本包装器，并把它们放到 DinD 容器内的 `/coast-supervisor/`。每个包装器会跟踪其 PID，将输出重定向到日志文件，并以 shell 循环的方式实现重启策略。这里没有 Docker Compose、没有内部 Docker 镜像，也没有服务之间的容器级隔离。

`coast ps` 通过检查 PID 是否存活来判断状态，而不是查询 Docker；`coast logs` 通过 tail 日志文件来输出日志，而不是调用 `docker compose logs`。日志输出格式与 compose 的 `service | line` 格式一致，因此 Coastguard 的 UI 无需改动即可工作。

## 端口

端口配置与基于 compose 的 Coast 完全相同。在 `[ports]` 中定义你的服务监听的端口:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[Dynamic ports](PORTS.md) 会在 `coast run` 时分配，[`coast checkout`](CHECKOUT.md) 会照常交换规范端口。唯一的区别是:服务之间没有 Docker 网络——它们都直接绑定到容器的 loopback 或 `0.0.0.0`。

## 分支切换

当你在裸服务的 Coast 上运行 `coast assign` 时，会发生以下过程:

1. 通过 SIGTERM 停止所有正在运行的服务
2. 工作树切换到新分支
3. 重新运行 install 命令（例如 `npm install` 会获取新分支的依赖）
4. 重启所有服务

这等同于使用 compose 时发生的事情——`docker compose down`、切换分支、重建、`docker compose up`——只是这里运行的是 shell 进程而不是容器。

## 限制

- **没有健康检查。** Coast 无法像对定义了健康检查的 compose 服务那样等待裸服务“健康”。Coast 会启动进程，但无法知道它何时真正就绪。
- **服务之间没有隔离。** 所有进程在 Coast 容器内共享同一文件系统与进程命名空间。行为异常的服务可能影响其他服务。
- **没有构建缓存。** Docker Compose 构建会按层缓存。裸服务的 `install` 命令会在每次 assign 时从头执行。
- **崩溃恢复很基础。** 重启策略使用带指数退避的 shell 循环。它不是 systemd 或 supervisord 那样的进程监督器。
- **服务不支持 `[omit]` 或 `[unset]`。** Coastfile 的类型组合适用于 compose 服务，但裸服务不支持通过带类型的 Coastfile 省略单个服务。

## 迁移到 Compose

当你准备好容器化时，迁移路径很直接:

1. 为每个服务编写一个 `Dockerfile`
2. 创建一个引用这些 Dockerfile 的 `docker-compose.yml`
3. 将 Coastfile 中的 `[services.*]` 段落替换为一个指向你的 compose 文件的 `compose` 字段
4. 移除那些现在由 Dockerfile 处理的 `[coast.setup]` 包
5. 使用 [`coast build`](BUILDS.md) 重新构建

你的端口映射、[volumes](VOLUMES.md)、[shared services](SHARED_SERVICES.md) 和 [secrets](SECRETS.md) 配置都会原封不动地沿用。唯一变化的是服务本身的运行方式。

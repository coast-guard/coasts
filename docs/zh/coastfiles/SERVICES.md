# 裸服务

> **注意:** 裸服务以普通进程的形式直接在 Coast 容器内运行——它们并未容器化。如果你的服务已经 Docker 化，请改用 `compose`。裸服务最适合简单场景:你希望跳过编写 Dockerfile 和 docker-compose.yml 的开销。

`[services.*]` 部分定义了 Coast 在 DinD 容器内直接运行的进程，而不使用 Docker Compose。这是使用 `compose` 文件的替代方案——你不能在同一个 Coastfile 中同时使用两者。

裸服务由 Coast 进行监管，包含日志捕获以及可选的重启策略。关于裸服务如何工作、其限制，以及何时迁移到 compose 的更深入背景，请参见 [Bare Services](../concepts_and_terminology/BARE_SERVICES.md)。

## 定义服务

每个服务都是 `[services]` 下一个具名的 TOML 部分。`command` 字段是必需的。

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command`（必需）

要运行的 shell 命令。不能为空或仅包含空白字符。

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

服务监听的端口。用于健康检查以及端口转发集成。如果指定，必须为非零值。

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

进程退出时的重启策略。默认值为 `"no"`。

- `"no"` — 不重启
- `"on-failure"` — 仅当进程以非零退出码退出时重启
- `"always"` — 总是重启

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

在启动服务之前要运行的命令（例如安装依赖）。可以是单个字符串或字符串数组。

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## 与 compose 的互斥

一个 Coastfile 不能同时定义 `compose` 和 `[services]`。如果你在 `[coast]` 中有一个 `compose` 字段，那么添加任何 `[services.*]` 部分都会报错。每个 Coastfile 请选择一种方式。

如果你需要一些服务通过 compose 容器化、另一些以裸服务运行，请对所有服务都使用 compose——如何从裸服务迁移到 compose，请参见 [Bare Services 中的迁移指南](../concepts_and_terminology/BARE_SERVICES.md)。

## 示例

### 单服务 Next.js 应用

```toml
[coast]
name = "my-frontend"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### 带后台 worker 的 Web 服务器

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### 带多步骤安装的 Python 服务

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```

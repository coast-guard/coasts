# 项目与设置

`[coast]` 部分是 Coastfile 中唯一必需的部分。它用于标识项目并配置 Coast 容器的创建方式。可选的 `[coast.setup]` 子部分允许你在构建时在容器内安装软件包并运行命令。

## `[coast]`

### `name`（必需）

项目的唯一标识符。用于容器名称、卷名称、状态跟踪以及 CLI 输出。

```toml
[coast]
name = "my-app"
```

### `compose`

Docker Compose 文件的路径。相对路径会相对于项目根目录解析（包含 Coastfile 的目录，或在设置了 `root` 时相对于 `root`）。

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

如果省略，Coast 容器会在不运行 `docker compose up` 的情况下启动。你可以使用 [裸服务](SERVICES.md)，或通过 `coast exec` 直接与容器交互。

你不能在同一个 Coastfile 中同时设置 `compose` 和 `[services]`。

### `runtime`

要使用的容器运行时。默认为 `"dind"`（Docker-in-Docker）。

- `"dind"` — 使用 `--privileged` 的 Docker-in-Docker。唯一经过生产环境验证的运行时。参见 [运行时与服务](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md)。
- `"sysbox"` — 使用 Sysbox 运行时替代特权模式。需要已安装 Sysbox。
- `"podman"` — 使用 Podman 作为内部容器运行时。

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

覆盖项目根目录。默认情况下，项目根目录是包含 Coastfile 的目录。相对路径会相对于 Coastfile 所在目录解析；绝对路径将按原样使用。

```toml
[coast]
name = "my-app"
root = "../my-project"
```

这并不常见。大多数项目会将 Coastfile 放在实际的项目根目录下。

### `worktree_dir`

为 Coast 实例创建 git worktree 的目录。默认为 `".coasts"`。相对路径会相对于项目根目录解析。

```toml
[coast]
name = "my-app"
worktree_dir = ".worktrees"
```

如果该目录是相对路径且位于项目内部，Coast 会自动将其添加到 `.gitignore`。

### `autostart`

当使用 `coast run` 创建 Coast 实例时，是否自动运行 `docker compose up`（或启动裸服务）。默认为 `true`。

当你希望容器保持运行但希望手动启动服务时，将其设置为 `false` —— 这对测试运行器变体很有用，你可以按需触发测试。

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

从 `[ports]` 部分指定一个端口名称，用于快速链接和子域路由。其值必须与 `[ports]` 中定义的键匹配。

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

参见 [主端口与 DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md)，了解它如何启用子域路由与 URL 模板。

## `[coast.setup]`

自定义 Coast 容器本身——安装工具、运行构建步骤，以及生成配置文件。`[coast.setup]` 中的所有内容都在 DinD 容器内运行（而不是在你的 compose 服务内）。

### `packages`

要安装的 APK 软件包。由于基础 DinD 镜像基于 Alpine Linux，这些是 Alpine Linux 软件包。

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

在构建期间按顺序执行的 shell 命令。用于安装无法作为 APK 软件包获取的工具。

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

在容器内创建的文件。每个条目包含 `path`（必需，必须是绝对路径）、`content`（必需）以及可选的 `mode`（3-4 位八进制字符串）。

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

文件条目的校验规则:

- `path` 必须是绝对路径（以 `/` 开头）
- `path` 不能包含 `..` 组件
- `path` 不能以 `/` 结尾
- `mode` 必须是 3 或 4 位的八进制字符串（例如 `"600"`, `"0644"`）

## 完整示例

一个为 Go 与 Node.js 开发设置好的 Coast 容器:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = ".worktrees"
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

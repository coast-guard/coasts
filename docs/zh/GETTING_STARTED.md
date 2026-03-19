# Coasts 入门

```youtube
Je921fgJ4RY
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## 安装

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*如果你决定不运行 `coast daemon install`，那么你需要负责每一次都手动通过 `coast daemon start` 启动守护进程。*

## 要求

- macOS 或 Linux
- 在 macOS 上使用 Docker Desktop，或在 Linux 上使用带 Compose 插件的 Docker Engine
- 使用 Git 的项目
- Node.js
- `socat`（在 macOS 上运行 `brew install socat`，在 Ubuntu 上运行 `sudo apt install socat`）

```text
Linux note: Dynamic ports work out of the box on Linux.
If you need canonical ports below `1024`, see the checkout docs for the required host configuration.
```

## 在项目中设置 Coasts

在项目根目录添加一个 Coastfile。安装时请确保你不在 worktree 上。

```text
my-project/
├── Coastfile              <-- this is what Coast reads
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

`Coastfile` 指向你现有的本地开发资源，并添加 Coasts 特有的配置——完整 schema 请参阅 [Coastfiles documentation](coastfiles/README.md):

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile 是一个轻量级的 TOML 文件，*通常*会指向你现有的 `docker-compose.yml`（也适用于非容器化的本地开发设置），并描述为了并行运行你的项目所需的修改——端口映射、卷策略以及密钥。将它放在你的项目根目录。

为你的项目创建 Coastfile 的最快方式是让你的编码智能体来完成。

Coasts CLI 内置了一个 prompt，可向任何 AI 智能体讲解完整的 Coastfile schema 和 CLI。把它复制到你智能体的聊天中，它会分析你的项目并生成一个 Coastfile。

```prompt-copy
installation_prompt.txt
```

你也可以通过运行 `coast installation-prompt` 从 CLI 获取相同的输出。

## 你的第一个 Coast

在启动你的第一个 Coast 之前，先关闭任何正在运行的开发环境。如果你使用 Docker Compose，请运行 `docker-compose down`。如果你有本地开发服务器在运行，请停止它们。Coasts 会管理自己的端口，并且会与任何已在监听的服务产生冲突。

当你的 Coastfile 准备好后:

```bash
coast build
coast run dev-1
```

检查你的实例是否在运行:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

查看你的服务在监听哪些端口:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

每个实例都会获得自己的一组动态端口，这样多个实例就可以并排运行。要将实例映射回你项目的规范端口，请将其 checkout:

```bash
coast checkout dev-1
```

这意味着运行时现在已被 checkout，你项目的规范端口（例如 `3000`、`5432`）将路由到这个 Coast 实例。

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

为你的项目打开 Coastguard 可观测性 UI:

```bash
coast ui
```

## 接下来做什么？

- 为你的主机智能体设置一个 [skill for your host agent](SKILLS_FOR_HOST_AGENTS.md)，让它知道如何与 Coasts 交互

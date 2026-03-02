# Coasts 入门

如果你还没有完成，请先完成下面的安装和要求。然后本指南将带你了解如何在项目中使用 Coast。

## 安装

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*如果你决定不运行 `coast daemon install`，那么你需要负责每一次都手动通过 `coast daemon start` 启动守护进程。*

## 要求

- macOS
- Docker Desktop
- 使用 Git 的项目
- Node.js
- `socat`（在 macOS 上运行 `brew install socat`）

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
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

Coasts CLI 内置了一个 prompt，可向任何 AI 智能体讲解完整的 Coastfile schema 和 CLI。你可以在这里查看: [installation_prompt.txt](installation_prompt.txt)

把它直接传给你的智能体，或者复制 [installation prompt](installation_prompt.txt) 并粘贴到智能体的聊天中:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

该 prompt 覆盖 Coastfile TOML 格式、卷策略、密钥注入，以及所有相关 CLI 命令。你的智能体会分析你的项目并生成一个 Coastfile。

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

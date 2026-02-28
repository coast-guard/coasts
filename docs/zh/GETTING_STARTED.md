# Coasts 入门指南

如果您还没有完成下面的安装与要求，请先完成它们。然后本指南将带您了解如何在项目中使用 Coast。

## 安装

- `brew install coast`
- `coast daemon install`

*如果您决定不运行 `coast daemon install`，那么您需要负责在每一次都手动使用 `coast daemon start` 启动守护进程。*

## 要求

- macOS
- Docker Desktop
- 使用 Git 的项目
- Node.js
- `socat` *(通过 `brew install coast` 安装时，作为 Homebrew 的 `depends_on` 依赖一并安装)*

```text
Linux 注意:我们尚未在 Linux 上测试过 Coasts，但计划支持 Linux。
您今天可以尝试在 Linux 上运行 Coasts，但我们不保证它能正确工作。
```

## 在项目中设置 Coasts

在项目根目录添加一个 Coastfile。安装时请确保您不在 worktree 中。

```text
my-project/
├── Coastfile              <-- this is what Coast reads
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

`Coastfile` 指向您现有的本地开发资源，并添加 Coasts 专用的配置——完整 schema 请参阅 [Coastfiles 文档](coastfiles/README.md):

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile 是一个轻量级的 TOML 文件，*通常*会指向您现有的 `docker-compose.yml`（它也支持非容器化的本地开发设置），并描述让您的项目能够并行运行所需的修改——端口映射、卷策略以及密钥。请将其放在项目根目录。

为您的项目创建 Coastfile 的最快方式，是让您的编码代理来完成。

Coasts CLI 内置了一个 prompt，可向任何 AI 代理讲解完整的 Coastfile schema 与 CLI。您可以在这里查看:[installation_prompt.txt](installation_prompt.txt)

您可以直接把它传给您的代理，或复制 [installation prompt](installation_prompt.txt) 并粘贴到代理的聊天中:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

该 prompt 涵盖 Coastfile TOML 格式、卷策略、密钥注入以及所有相关 CLI 命令。您的代理会分析您的项目并生成一个 Coastfile。

## 您的第一个 Coast

在启动第一个 Coast 之前，请先停止任何正在运行的开发环境。如果您使用 Docker Compose，请运行 `docker-compose down`。如果您有本地开发服务器在运行，请停止它们。Coasts 会管理自己的端口，并会与任何已在监听的服务发生冲突。

当您的 Coastfile 准备好后:

```bash
coast build
coast run dev-1
```

检查您的实例是否在运行:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

查看您的服务在监听哪些端口:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

每个实例都会获得自己的一组动态端口，因此可以并排运行多个实例。要将某个实例映射回您项目的规范端口，请将其检出:

```bash
coast checkout dev-1
```

这表示运行时现在已被检出，您项目的规范端口（例如 `3000`、`5432`）将路由到这个 Coast 实例。

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

为您的项目打开 Coastguard 可观测性 UI:

```bash
coast ui
```

## 下一步？

- 为您的宿主代理设置一个 [skill](SKILLS_FOR_HOST_AGENTS.md)，让它知道如何与 Coasts 交互

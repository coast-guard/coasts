# Agent Shell

> **在大多数工作流中，你不需要将你的编码代理容器化。** 因为 Coast 与你的宿主机共享[文件系统](../concepts_and_terminology/FILESYSTEM.md)，最简单的方法是在宿主机上运行代理，并对集成测试等运行时开销较大的任务使用 [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md)。Agent shell 适用于你特别希望代理在容器内运行的场景——例如，为其提供对内部 Docker 守护进程的直接访问，或完全隔离其环境。

`[agent_shell]` 部分用于配置一个代理 TUI——例如 Claude Code 或 Codex——在 Coast 容器内运行。配置存在时，Coast 会在实例启动时自动生成一个持久化 PTY 会话来运行所配置的命令。

关于 agent shell 如何工作的全貌——活动代理模型、发送输入、生命周期与恢复——请参阅 [Agent Shells](../concepts_and_terminology/AGENT_SHELLS.md)。

## Configuration

该部分只有一个必填字段:`command`。

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (required)

在代理 PTY 中运行的 shell 命令。这通常是你通过 `[coast.setup]` 安装的编码代理 CLI。

该命令在 DinD 容器内的 `/workspace`（项目根目录）中运行。它不是 compose 服务——它与 compose 堆栈或裸服务并行运行，而不是在它们内部运行。

## Lifecycle

- agent shell 会在 `coast run` 时自动生成。
- 在 [Coastguard](../concepts_and_terminology/COASTGUARD.md) 中，它会显示为一个无法关闭的持久化 “Agent” 标签页。
- 如果代理进程退出，Coast 可以将其重新生成。
- 你可以通过 `coast agent-shell input` 向正在运行的 agent shell 发送输入。

## Examples

### Claude Code

在 `[coast.setup]` 中安装 Claude Code，通过 [secrets](SECRETS.md) 配置凭据，然后设置 agent shell:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git", "bash"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "mkdir -p /root/.claude",
]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "cd /workspace; exec claude --dangerously-skip-permissions --effort high"
```

### Simple agent shell

一个用于测试该功能是否可用的最小 agent shell:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```

# Agent Shells

Agent shells 是 Coast 内部的 shell，它们会直接打开到一个 agent TUI 运行时——Claude Code、Codex，或任何 CLI agent。你可以在 Coastfile 中通过一个 `[agent_shell]` 段来配置它们，然后 Coast 会在 DinD 容器内生成（spawn）该 agent 进程。

**对于大多数使用场景，你不应该这样做。** 相反，请在宿主机上运行你的编码 agent。共享的[文件系统](FILESYSTEM.md)意味着宿主机侧的 agent 可以像平常一样编辑代码，同时调用 [`coast logs`](LOGS.md)、[`coast exec`](EXEC_AND_DOCKER.md) 和 [`coast ps`](RUNTIMES_AND_SERVICES.md) 来获取运行时信息。Agent shells 会增加凭据挂载、OAuth 复杂性和生命周期复杂性；除非你有一个必须将 agent 本身容器化的特定理由，否则不需要这些。

## The OAuth Problem

如果你在使用 Claude Code、Codex 或类似通过 OAuth 进行认证的工具，那么该 token 是为你的宿主机签发的。当同一个 token 从 Linux 容器内部使用时——不同的 user agent、不同的环境——提供方可能会标记或吊销它。你会遇到难以调试的间歇性认证失败。

对于容器化的 agents，基于 API key 的认证是更安全的选择。将 key 作为 Coastfile 中的一个[secret](SECRETS.md) 设置，并将其注入到容器环境中。

如果无法使用 API key，你可以将 OAuth 凭据挂载到 Coast 中（见下方的 Configuration 章节），但要预期会有摩擦。在 macOS 上，如果你使用 `keychain` secret extractor 来拉取 OAuth token，那么每次 `coast build` 都会提示输入你的 macOS 钥匙串密码。这会让构建过程变得繁琐，尤其是在频繁重建时。Keychain 弹窗是 macOS 的安全要求，无法绕过。

## Configuration

在你的 Coastfile 中添加一个 `[agent_shell]` 段，并提供要运行的命令:

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

该命令会在 DinD 容器内的 `/workspace` 执行。Coast 会在容器内创建一个 `coast` 用户，把凭据从 `/root/.claude/` 复制到 `/home/coast/.claude/`，并以该用户运行命令。如果你的 agent 需要将凭据挂载进容器，请使用带文件注入的 `[secrets]`（见 [Secrets and Extractors](SECRETS.md)）以及用 `[coast.setup]` 安装 agent CLI:

```toml
[coast.setup]
run = ["npm install -g @anthropic-ai/claude-code"]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "claude --dangerously-skip-permissions"
```

如果配置了 `[agent_shell]`，Coast 会在实例启动时自动生成一个 shell。该配置会通过 `extends` 继承，并且可以按 [Coastfile type](COASTFILE_TYPES.md) 进行覆盖。

## The Active Agent Model

每个 Coast 实例可以有多个 agent shell，但任意时刻只有一个是 **active**。active shell 是未指定 `--shell` ID 的命令的默认目标。

```bash
coast agent-shell dev-1 ls

  SHELL  STATUS   ACTIVE
  1      running  ★
  2      running
```

切换 active shell:

```bash
coast agent-shell dev-1 activate 2
```

你不能关闭 active shell——必须先激活另一个。这能防止你意外杀掉正在交互的 shell。

在 Coastguard 中，agent shells 会在 Exec 面板里以标签页的形式出现，并带有 active/inactive 徽标。点击标签页可查看其终端；使用下拉菜单来激活、生成或关闭 shells。

![Agent shell in Coastguard](../../assets/coastguard-agent-shell.png)
*一个在 Coast 实例内运行 Claude Code 的 agent shell，可从 Coastguard 的 Exec 标签访问。*

## Sending Input

以编程方式驱动容器化 agent 的主要方式是 `coast agent-shell input`:

```bash
coast agent-shell dev-1 input "fix the failing test in auth.test.ts"
```

这会将文本写入 active agent 的 TUI 并按下 Enter。agent 会像你在终端里输入一样接收它。

选项:

- `--no-send` — 写入文本但不按 Enter。用于构建部分输入或导航 TUI 菜单。
- `--shell <id>` — 指定某个 shell，而不是 active shell。
- `--show-bytes` — 打印发送的精确字节，用于调试。

在底层，输入会直接写入 PTY master 文件描述符。文本和 Enter 按键会作为两次独立写入发送，中间间隔 25ms，以避免某些 TUI 框架在接收快速输入时出现的粘贴模式伪影。

## Other Commands

```bash
coast agent-shell dev-1 spawn              # create a new shell
coast agent-shell dev-1 spawn --activate   # create and immediately activate
coast agent-shell dev-1 tty                # attach interactive TTY to active shell
coast agent-shell dev-1 tty --shell 2      # attach to a specific shell
coast agent-shell dev-1 read-output        # read full scrollback buffer
coast agent-shell dev-1 read-last-lines 50 # read last 50 lines of output
coast agent-shell dev-1 session-status     # check if the shell process is alive
```

`tty` 会给你一个实时的交互式会话——你可以直接在 agent 的 TUI 中键入。使用标准终端转义序列断开连接。`read-output` 和 `read-last-lines` 是非交互式的，会返回文本，这对脚本与自动化很有用。

## Lifecycle and Recovery

在 Coastguard 中，agent shell 会话在页面导航之间保持持久。滚动缓冲区（最多 512KB）会在你重新连接到某个标签页时回放。

当你用 `coast stop` 停止一个 Coast 实例时，所有 agent shell 的 PTY 进程都会被杀掉，并清理它们的数据库记录。如果配置了 `[agent_shell]`，`coast start` 会自动生成一个新的 agent shell。

在 daemon 重启后，之前运行的 agent shells 会显示为已死亡。系统会自动检测——如果 active shell 已死亡，则第一个仍存活的 shell 会被提升为 active。如果没有任何 shell 存活，请用 `coast agent-shell spawn --activate` 生成一个新的。

## Who This Is For

Agent shells 面向的是 **围绕 Coasts 构建第一方集成的产品**——编排平台、agent 包装器，以及希望通过 `input`、`read-output`、`session-status` API 以编程方式管理容器化编码 agent 的工具。

对于通用的并行 agent 编码，请在宿主机上运行 agents。这样更简单，避免 OAuth 问题，绕开凭据挂载的复杂性，并能充分利用共享文件系统。你可以获得 Coast 的全部收益（隔离运行时、端口管理、worktree 切换），而不需要任何 agent 容器化的额外开销。

比 agent shells 更高一层的复杂度是将 [MCP servers](MCP_SERVERS.md) 挂载到 Coast 中，使容器化 agent 能访问工具。这会进一步扩大集成面，并在其他文档中单独覆盖。如果你需要，这个能力是存在的，但大多数用户不应使用。

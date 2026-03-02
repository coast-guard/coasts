# 面向主机代理的技能

如果你在使用 Coasts 的项目中使用 AI 编码代理（Claude Code、Codex、Conductor、Cursor 或类似工具），你的代理需要一个技能来教它如何与 Coast 运行时交互。否则，代理会编辑文件，但不知道如何运行测试、查看日志，或验证其更改是否在运行中的环境里生效。

本指南将带你完成该技能的设置。

## 为什么代理需要这个

Coasts 在你的主机与 Coast 容器之间共享[文件系统](concepts_and_terminology/FILESYSTEM.md)。你的代理在主机上编辑文件，而 Coast 内运行的服务会立刻看到这些更改。但代理仍然需要:

1. **发现它正在使用哪个 Coast 实例** — `coast lookup` 会根据代理的当前目录解析出实例。
2. **在 Coast 内运行命令** — 测试、构建以及其他运行时任务需要通过 `coast exec` 在容器内执行。
3. **读取日志并检查服务状态** — `coast logs` 和 `coast ps` 为代理提供运行时反馈。

下面的技能会教会代理这三点。

## 该技能

将以下内容添加到你代理现有的技能、规则或提示文件中。如果你的代理已经有关于运行测试或与开发环境交互的说明，这段内容应与它们并列——它教代理如何使用 Coasts 来进行运行时操作。

```text-copy
This project uses Coasts (containerized host) for isolated development environments.
Your code edits are automatically visible inside the running Coast — the filesystem
is shared between the host and the container.

=== ORIENTATION ===

Before running any runtime commands, discover which Coast instance matches your
current working directory:

  coast lookup

This prints the instance name, ports, URLs, and example commands. Use the instance
name from the output for all subsequent commands.

If you need deeper context on how Coasts work, read these docs:

  coast docs --path concepts_and_terminology/LOOKUP.md
  coast docs --path concepts_and_terminology/FILESYSTEM.md
  coast docs --path concepts_and_terminology/EXEC_AND_DOCKER.md
  coast docs --path concepts_and_terminology/LOGS.md

=== RUNNING COMMANDS ===

Use `coast exec` to run commands inside the Coast. The shell starts at the workspace
root (where the Coastfile is). cd to your target directory first:

  coast exec <instance> -- sh -c "cd <dir> && <command>"

Examples:

  coast exec dev-1 -- sh -c "cd src && npm test"
  coast exec dev-1 -- sh -c "cd backend && go test ./..."
  coast exec dev-1 -- sh -c "cd apps/web && npx playwright test"

=== RUNTIME FEEDBACK ===

Check service status:

  coast ps <instance>

Read service logs:

  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

=== TROUBLESHOOTING ===

If you encounter errors or unfamiliar behavior, search the Coast docs:

  coast search-docs "error message or description"

This uses semantic search — describe the problem in natural language and it will
find the relevant documentation.

=== RULES ===

- Always run `coast lookup` before your first runtime command in a session.
- Do not run services directly on the host. Use `coast exec` for all runtime tasks.
- File edits on the host are instantly visible inside the Coast. You do not need
  to copy files or rebuild after editing.
- If `coast lookup` returns no instances, the Coast may not be running. Suggest
  `coast run dev-1` or check `coast ls` for the project state.
```

## 将该技能添加到你的代理

最快的方法是让代理自己完成设置。在你的项目目录中运行以下命令之一:

```sh
# Claude Code
claude -p "$(coast skills-prompt)"

# Codex
codex "$(coast skills-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast skills-prompt)"
```

这会向代理提供技能文本以及将其写入自身配置文件（`CLAUDE.md`、`AGENTS.md`、`.cursor/rules/coast.md` 等）的指令。

### 手动设置

如果你更愿意自己添加该技能:

- **Claude Code:** 将技能文本添加到你项目的 `CLAUDE.md` 文件中。
- **Codex:** 将技能文本添加到你项目的 `AGENTS.md` 文件中。
- **Cursor:** 在项目根目录创建 `.cursor/rules/coast.md` 并粘贴技能文本。
- **其他代理:** 将技能文本粘贴到你的代理在启动时读取的任意项目级提示或规则文件中。

## 进一步阅读

- 阅读 [Coastfiles 文档](coastfiles/README.md)以了解完整的配置 schema
- 学习用于管理实例的 [Coast CLI](concepts_and_terminology/CLI.md) 命令
- 探索用于观察与控制 Coasts 的 Web UI:[Coastguard](concepts_and_terminology/COASTGUARD.md)
- 浏览 [概念与术语](concepts_and_terminology/README.md)，全面了解 Coasts 的工作方式

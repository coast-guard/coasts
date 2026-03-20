# 主机代理的技能

如果你在主机上使用 AI 编码代理，而你的应用运行在 Coasts 内部，那么你的代理通常需要两项 Coasts 特定的设置:

1. 在 harness 的项目说明文件或规则文件中加入始终启用的 Coast Runtime 部分
2. 在 harness 支持项目技能时，添加一个可复用的 Coast 工作流技能，例如 `/coasts`

如果缺少第一项，代理会编辑文件，但会忘记使用 `coast exec`。
如果缺少第二项，那么每次涉及 Coast 分配、日志和 UI 流程时，都必须在聊天中重新解释。

本指南会把设置讲得具体且聚焦于 Coasts:要创建哪个文件、其中应放入什么文本，以及这些内容如何因 harness 而异。

## 为什么代理需要这个

Coasts 在你的主机和 Coast 容器之间共享[文件系统](concepts_and_terminology/FILESYSTEM.md)。你的代理在主机上编辑文件，而 Coast 内运行的服务会立即看到这些更改。但代理仍然需要:

1. 发现哪个 Coast 实例与当前 checkout 匹配
2. 在该 Coast 内运行测试、构建和运行时命令
3. 从 Coast 读取日志和服务状态
4. 在尚未附加任何 Coast 时安全地处理 worktree 分配

## 各部分应放在哪里

- `AGENTS.md`、`CLAUDE.md` 或 `.cursor/rules/coast.md` —— 简短的 Coast 规则
  即使未调用任何技能，也应在每个任务中生效
- skill（`.agents/skills/...`、`.claude/skills/...` 或 `.cursor/skills/...`）
  —— 可复用的 Coast 工作流本身，例如 `/coasts`
- command file（`.claude/commands/...` 或 `.cursor/commands/...`）—— 可选的
  显式入口点，供支持它的 harness 使用；一个简单的做法是让命令复用该技能

如果一个仓库使用多个 harness，请将规范的 Coast 技能保存在一个位置，
并在需要的地方暴露出来。参见
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md)。

## 1. 始终启用的 Coast Runtime 规则

将以下代码块添加到 harness 的始终启用项目说明文件或规则文件中
（`AGENTS.md`、`CLAUDE.md`、`.cursor/rules/coast.md` 或等效文件）:

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

这个代码块应放在始终启用的文件中，因为这些规则应在每个任务中生效，
而不仅仅是在代理显式进入 `/coasts` 工作流时才生效。

## 2. 可复用的 `/coasts` 技能

当 harness 支持项目技能时，请将技能内容保存为技能目录中的
`SKILL.md`。完整的技能文本位于
[skills_prompt.txt](skills_prompt.txt)（如果使用 CLI 模式，请使用
`coast skills-prompt`）—— Coast Runtime 代码块之后的所有内容都是技能
内容，从 `---` frontmatter 开始。

如果你使用 Codex 或 OpenAI 特定的界面，还可以选择在技能旁边添加
`agents/openai.yaml`，用于显示元数据或调用策略。
这些元数据应与技能放在一起，而不是取代技能本身。

## Harness 快速开始

| Harness | 始终启用文件 | 可复用的 Coast 工作流 | 说明 |
|---------|----------------|-------------------------|-------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | 对于 Coast 文档，不建议单独使用项目命令文件。参见 [Codex](harnesses/CODEX.md)。 |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md` 是可选的，但逻辑应保留在技能中。参见 [Claude Code](harnesses/CLAUDE_CODE.md)。 |
| Cursor | `AGENTS.md` 或 `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` 或共享的 `.agents/skills/coasts/SKILL.md` | `.cursor/commands/coasts.md` 是可选的。`.cursor/worktrees.json` 用于 Cursor worktree 引导，而不是 Coast 策略。参见 [Cursor](harnesses/CURSOR.md)。 |
| Conductor | `CLAUDE.md` | 从 `CLAUDE.md` 开始；使用 Conductor 脚本和设置来处理 Conductor 特定行为 | 不要假设其完整支持 Claude Code 的项目命令行为。如果新命令没有出现，请彻底关闭并重新打开 Conductor。参见 [Conductor](harnesses/CONDUCTOR.md)。 |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | 这是这里功能最有限的 harness 界面。使用 Codex 风格的布局，不要为 Coast 文档虚构 T3 原生命令层。参见 [T3 Code](harnesses/T3_CODE.md)。 |

## 让代理自行完成设置

最快的方法是让代理自己写入正确的文件。将下面的提示复制到你的代理聊天中——它包含 Coast Runtime 代码块、`coasts` 技能代码块，以及每个部分应放在哪里的 harness 特定说明。

```prompt-copy
skills_prompt.txt
```

你也可以通过运行 `coast skills-prompt` 从 CLI 获取相同的输出。

## 手动设置

- **Codex:** 将 Coast Runtime 部分放入 `AGENTS.md`，然后将可复用的
  `coasts` 技能放入 `.agents/skills/coasts/SKILL.md`。
- **Claude Code:** 将 Coast Runtime 部分放入 `CLAUDE.md`，然后将可复用的
  `coasts` 技能放入 `.claude/skills/coasts/SKILL.md`。只有在你明确需要命令文件时，
  才添加 `.claude/commands/coasts.md`。
- **Cursor:** 如果你想要最具可移植性的说明，请将 Coast Runtime 部分放入 `AGENTS.md`；
  如果你想使用 Cursor 原生项目规则，则放入 `.cursor/rules/coast.md`。将可复用的
  `coasts` 工作流放入 `.cursor/skills/coasts/SKILL.md`（适用于仅使用 Cursor 的仓库），
  或放入 `.agents/skills/coasts/SKILL.md`（适用于与其他 harness 共享的仓库）。
  只有在你明确想要显式命令文件时，才添加 `.cursor/commands/coasts.md`。
- **Conductor:** 将 Coast Runtime 部分放入 `CLAUDE.md`。使用 Conductor
  Repository Settings 脚本来处理 Conductor 特定的引导或运行行为。
  如果你添加了命令但没有显示出来，请彻底关闭并重新打开应用。
- **T3 Code:** 使用与 Codex 相同的布局:`AGENTS.md` 加上
  `.agents/skills/coasts/SKILL.md`。这里应将 T3 Code 视为轻量的 Codex 风格
  harness，而不是单独的 Coast 命令界面。
- **多个 harness:** 将规范技能保存在
  `.agents/skills/coasts/SKILL.md` 中。Cursor 可以直接加载它；如有需要，
  可通过 `.claude/skills/coasts/` 将其暴露给 Claude Code。

## 延伸阅读

- 阅读 [Harnesses guide](harnesses/README.md) 了解各 harness 的矩阵
- 阅读 [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md) 了解共享布局模式
- 阅读 [Coastfiles documentation](coastfiles/README.md) 以了解完整的配置模式
- 学习用于管理实例的 [Coast CLI](concepts_and_terminology/CLI.md) 命令
- 探索 [Coastguard](concepts_and_terminology/COASTGUARD.md)，这是用于观察和控制你的 Coasts 的 Web UI

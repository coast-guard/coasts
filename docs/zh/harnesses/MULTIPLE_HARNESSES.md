# 多个 Harness

如果一个仓库会被多个 harness 使用，整合 Coasts 设置的一种方式是将共享的 `/coasts` 工作流保存在一个位置，并将各个 harness 特定的常驻规则保存在对应 harness 的文件中。

## 推荐布局

```text
AGENTS.md
CLAUDE.md
.cursor/rules/coast.md           # optional Cursor-native always-on rules
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md       # optional, thin, harness-specific
.claude/commands/coasts.md   # optional, thin, harness-specific
```

按如下方式使用此布局:

- `AGENTS.md` — 在 Codex 和 T3
  Code 中使用 Coasts 的简短常驻规则
- `.cursor/rules/coast.md` — 可选的 Cursor 原生常驻规则
- `CLAUDE.md` — 在 Claude Code
  和 Conductor 中使用 Coasts 的简短常驻规则
- `.agents/skills/coasts/SKILL.md` — 规范的可复用 `/coasts` 工作流
- `.agents/skills/coasts/agents/openai.yaml` — 可选的 Codex/OpenAI 元数据
- `.claude/skills/coasts` — 面向 Claude 的镜像或符号链接，当 Claude Code
  也需要相同 skill 时使用
- `.cursor/commands/coasts.md` — 可选的 Cursor 命令文件；一个简单的
  选项是让它复用同一个 skill
- `.claude/commands/coasts.md` — 可选的显式命令文件；一个简单的
  选项是让它复用同一个 skill

## 分步说明

1. 将 Coast Runtime 规则放入常驻指令文件中。
   - `AGENTS.md`、`CLAUDE.md` 或 `.cursor/rules/coast.md` 应回答
     “每个任务”规则:先运行 `coast lookup`，使用 `coast exec`，通过 `coast logs`
     读取日志，在没有匹配项时于执行 `coast assign` 或 `coast run` 前先询问。
2. 为 Coasts 创建一个规范 skill。
   - 将可复用的 `/coasts` 工作流放入 `.agents/skills/coasts/SKILL.md`。
   - 在该 skill 中直接使用 Coast CLI:`coast lookup`，
     `coast ls`、`coast run`、`coast assign`、`coast unassign`、
     `coast checkout` 和 `coast ui`。
3. 仅在 harness 需要不同路径时暴露该 skill。
   - Codex、T3 Code 和 Cursor 都可以直接使用 `.agents/skills/`。
   - Claude Code 需要 `.claude/skills/`，因此请将该规范
     skill 镜像或符号链接到该位置。
4. 仅当你想要一个显式的 `/coasts` 入口点时才添加命令文件。
   - 如果你创建 `.claude/commands/coasts.md` 或
     `.cursor/commands/coasts.md`，一个简单的选项是让该命令
     复用同一个 skill。
   - 如果你给该命令提供自己独立的说明，那么你就需要维护
     该工作流的第二份副本。
5. 将 Conductor 特定设置保留在 Conductor 中，而不是放在 skill 中。
   - 对于属于 Conductor 本身的引导或运行行为，请使用 Conductor Repository Settings 脚本。
   - 将 Coasts 策略以及 `coast` CLI 的使用保留在 `CLAUDE.md` 和共享 skill 中。

## 具体的 `/coasts` 示例

一个好的共享 `coasts` skill 应完成三项工作:

1. `Use Existing Coast`
   - 运行 `coast lookup`
   - 如果存在匹配项，则使用 `coast exec`、`coast ps` 和 `coast logs`
2. `Manage Assignment`
   - 运行 `coast ls`
   - 提供 `coast run`、`coast assign`、`coast unassign` 或
     `coast checkout`
   - 在复用或干扰现有槽位之前先询问
3. `Open UI`
   - 运行 `coast ui`

这才是 `/coasts` 工作流应放置的正确位置。常驻文件应
只保存那些即使从未调用该 skill 也必须适用的简短规则。

## 符号链接模式

如果你希望 Claude Code 复用与 Codex、T3 Code 或 Cursor 相同的 skill，
一种选择是使用符号链接:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

如果你的团队不想使用符号链接，提交一个镜像副本也完全可以。主要目标只是避免副本之间出现不必要的偏差。

## 各 harness 的注意事项

- Claude Code:项目 skills 和可选项目命令都有效，但
  请将逻辑保留在 skill 中。
- Cursor:使用 `AGENTS.md` 或 `.cursor/rules/coast.md` 放置简短的 Coast
  Runtime 规则，使用 skill 存放可复用工作流，并将
  `.cursor/commands` 保持为可选。
- Conductor:首先将其视为 `CLAUDE.md` 加 Conductor 脚本和设置。
  如果你添加了命令但它没有出现，请先完全关闭并重新打开应用，
  然后再检查一次。
- T3 Code:这是这里最轻量的 harness 表面。使用 Codex 风格的
  `AGENTS.md` 加 `.agents/skills` 模式，不要为关于 Coasts 的文档
  另行发明单独的 T3 专用命令布局。
- Codex:保持 `AGENTS.md` 简短，并将可复用工作流放入
  `.agents/skills`。

# Lookup

`coast lookup` 用于发现与调用者当前工作目录对应的哪些 Coast 实例正在运行。这是主机侧代理应当运行的第一个命令，用来让自己定位——“我在这里编辑代码，应该与哪个/哪些 Coast 交互？”

```bash
coast lookup
```

Lookup 会检测你是否在 [worktree](ASSIGN.md) 内或位于项目根目录，向守护进程查询匹配的实例，并输出包含端口、URL 以及示例命令的结果。

## Why This Exists

在主机上运行的 AI 编码代理（Cursor、Claude Code、Codex 等）通过 [shared filesystem](FILESYSTEM.md) 编辑文件，并调用 Coast CLI 命令来执行运行时操作。但代理首先需要回答一个基本问题:**哪个 Coast 实例对应于我正在工作的目录？**

如果没有 `coast lookup`，代理就必须运行 `coast ls`，解析完整的实例表，判断自己位于哪个 worktree 中，然后交叉比对。`coast lookup` 将这些步骤合并为一步，并返回代理可直接消费的结构化输出。

对于使用 Coast 的代理工作流，这个命令应当包含在任何顶层的 SKILL.md、AGENTS.md 或规则文件中。它是代理用来发现其运行时上下文的入口点。

## Output Modes

### Default (human-readable)

```bash
coast lookup
```

```text
Coast instances for worktree feature/oauth (my-app):

  dev-1  running  ★ checked out

  Primary URL:  http://dev-1.localhost:62217

  SERVICE              CANONICAL       DYNAMIC
  ★ web                3000            62217
    api                8080            63889
    postgres           5432            55681

  Examples (exec starts at the workspace root where your Coastfile is, cd to your target directory first):
    coast exec dev-1 -- sh -c "cd <dir> && <command>"
    coast logs dev-1 --service <service>
    coast ps dev-1
```

示例部分会提醒代理（以及人类）`coast exec` 是从工作区根目录开始的——也就是 Coastfile 所在的目录。要在子目录中运行命令，需要在 exec 内部先 `cd` 到该目录。

### Compact (`--compact`)

返回实例名称的 JSON 数组。为脚本和代理工具而设计，它们只需要知道应当针对哪些实例。

```bash
coast lookup --compact
```

```text
["dev-1"]
```

同一个 worktree 上有多个实例:

```text
["dev-1","dev-2"]
```

没有匹配项:

```text
[]
```

### JSON (`--json`)

以美化打印的 JSON 返回完整的结构化响应。为需要以机器可读格式获取端口、URL 和状态的代理而设计。

```bash
coast lookup --json
```

```json
{
  "project": "my-app",
  "worktree": "feature/oauth",
  "project_root": "/Users/dev/my-app",
  "instances": [
    {
      "name": "dev-1",
      "status": "Running",
      "checked_out": true,
      "branch": "feature/oauth",
      "primary_url": "http://dev-1.localhost:62217",
      "ports": [
        { "logical_name": "web", "canonical_port": 3000, "dynamic_port": 62217, "is_primary": true },
        { "logical_name": "api", "canonical_port": 8080, "dynamic_port": 63889, "is_primary": false }
      ]
    }
  ]
}
```

## How It Resolves

Lookup 会从当前工作目录向上查找最近的 Coastfile，然后确定你所在的 worktree:

1. 如果你的 cwd 位于 `{project_root}/{worktree_dir}/{name}/...` 之下，lookup 会找到分配给该 worktree 的实例。
2. 如果你的 cwd 位于项目根目录（或任何不在 worktree 内的目录），lookup 会找到 **未分配 worktree** 的实例——也就是仍然指向项目根目录的实例。

这意味着 lookup 也能从子目录工作。如果你在 `my-app/.coasts/feature-oauth/src/api/` 中，lookup 仍会将 `feature-oauth` 解析为 worktree。

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | 找到一个或多个匹配的实例 |
| 1 | 没有匹配的实例（结果为空） |

这使得 lookup 可用于 shell 条件判断:

```bash
if coast lookup > /dev/null 2>&1; then
  coast exec dev-1 -- sh -c "cd src && npm test"
fi
```

## For Agent Workflows

典型的代理集成模式:

1. 代理开始在某个 worktree 目录中工作。
2. 代理运行 `coast lookup` 以发现实例名称、端口、URL 和示例命令。
3. 代理在后续所有 Coast 命令中使用该实例名称:`coast exec`、`coast logs`、`coast ps`。

```text
┌─── Agent (host machine) ────────────────────────────┐
│                                                      │
│  1. coast lookup                                     │
│       → instance names, ports, URLs, examples        │
│  2. coast exec dev-1 -- sh -c "cd src && npm test"   │
│  3. coast logs dev-1 --service web --tail 50         │
│  4. coast ps dev-1                                   │
│                                                      │
└──────────────────────────────────────────────────────┘
```

如果代理需要跨多个 worktree 工作，它会在每个 worktree 目录中运行 `coast lookup`，以为每个上下文解析出正确的实例。

另请参见 [Filesystem](FILESYSTEM.md) 了解主机代理如何与 Coast 交互，参见 [Assign and Unassign](ASSIGN.md) 了解 worktree 概念，以及参见 [Exec & Docker](EXEC_AND_DOCKER.md) 了解如何在 Coast 内运行命令。

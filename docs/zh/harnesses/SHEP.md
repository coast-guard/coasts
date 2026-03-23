# Shep

## 快速设置

需要 [Coast CLI](../GETTING_STARTED.md)。将此提示复制到你的
代理聊天中，以自动设置 Coasts:

```prompt-copy
shep_setup_prompt.txt
```

你也可以从 CLI 获取技能内容:`coast skills-prompt`。

设置完成后，**退出并重新打开你的编辑器**，以使新技能和项目
说明生效。

---

[Shep](https://shep-ai.github.io/cli/) 会在 `~/.shep/repos/{hash}/wt/{branch-slug}` 创建工作树。该 hash 是仓库绝对路径的 SHA-256 的前 16 个十六进制字符，因此它对每个仓库都是确定的，但不透明。给定仓库的所有工作树共享相同的 hash，并通过 `wt/{branch-slug}` 子目录进行区分。

在 Shep CLI 中，`shep feat show <feature-id>` 会打印工作树路径，或者
`ls ~/.shep/repos` 会列出每个仓库对应的 hash 目录。

由于每个仓库的 hash 都不同，Coasts 使用 **glob 模式** 来发现
shep 工作树，而不需要用户将 hash 硬编码。

## 设置

将 `~/.shep/repos/*/wt` 添加到 `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

`*` 匹配每个仓库对应的 hash 目录。运行时 Coasts 会展开该 glob，
找到匹配的目录（例如 `~/.shep/repos/a21f0cda9ab9d456/wt`），并
将其绑定挂载到容器中。有关 glob
模式的完整细节，请参阅
[工作树目录](../coastfiles/WORKTREE_DIR.md)。

更改 `worktree_dir` 后，必须**重新创建**现有实例，绑定挂载才会生效:

```bash
coast rm my-instance
coast build
coast run my-instance
```

工作树列表会立即更新（Coasts 会读取新的 Coastfile），但
分配到 Shep 工作树需要容器内存在该绑定挂载。

## Coasts 指导内容放在哪里

Shep 底层封装了 Claude Code，因此请遵循 Claude Code 的约定:

- 将简短的 Coast Runtime 规则放在 `CLAUDE.md`
- 将可复用的 `/coasts` 工作流放在 `.claude/skills/coasts/SKILL.md` 或
  共享的 `.agents/skills/coasts/SKILL.md`
- 如果此仓库还使用其他 harness，请参阅
  [多个 Harness](MULTIPLE_HARNESSES.md) 和
  [宿主代理的 Skills](../SKILLS_FOR_HOST_AGENTS.md)

## Coasts 的作用

- **运行** -- `coast run <name>` 从最新构建创建一个新的 Coast 实例。使用 `coast run <name> -w <worktree>` 可一步创建并分配一个 Shep 工作树。参见 [运行](../concepts_and_terminology/RUN.md)。
- **绑定挂载** -- 在容器创建时，Coasts 会解析 glob
  `~/.shep/repos/*/wt`，并将每个匹配的目录挂载到容器中的
  `/host-external-wt/{index}`。
- **发现** -- `git worktree list --porcelain` 的作用域是仓库级别，因此只有
  属于当前项目的工作树会显示出来。
- **命名** -- Shep 工作树使用命名分支，因此它们会在 Coasts UI 和 CLI 中按分支
  名显示（例如，`feat-green-background`）。
- **分配** -- `coast assign` 会从外部绑定挂载路径重新挂载 `/workspace`。
- **Gitignored 同步** -- 在宿主文件系统上使用绝对路径运行，无需绑定挂载即可工作。
- **孤儿检测** -- git 监视器会递归扫描外部目录，
  并通过 `.git` gitdir 指针进行过滤。如果 Shep 删除了某个
  工作树，Coasts 会自动取消分配该实例。

## 示例

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `~/.shep/repos/*/wt` -- Shep（外部，通过 glob 展开进行绑定挂载）

## Shep 路径结构

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

关键点:
- 相同仓库 = 每次都是相同的 hash（确定性，不是随机）
- 不同仓库 = 不同的 hashes
- 路径分隔符在哈希之前会规范化为 `/`
- 可通过 `shep feat show <feature-id>` 或 `ls ~/.shep/repos` 找到该 hash

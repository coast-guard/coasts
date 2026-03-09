# 文件系统

你的主机与每个 Coast 实例共享同一份项目文件。主机项目根目录以读写方式挂载到 DinD 容器内的 `/host-project`，并且 Coast 将当前激活的工作树以 bind-mount 的方式挂载到 `/workspace`。这使得在主机上运行的代理可以编辑代码，而 Coast 内部的服务能够实时拾取变更。

## 共享挂载

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

当容器创建时，主机项目根目录会以读写方式挂载到 [DinD 容器](RUNTIMES_AND_SERVICES.md) 内的 `/host-project`。容器启动后，容器内会执行 `mount --bind /host-project /workspace` 来创建工作用的 `/workspace` 路径，并启用共享挂载传播（`mount --make-rshared`），因此将 `/workspace` 的子目录以 bind-mount 方式挂载的内部 compose 服务能够看到正确的内容。

这种两阶段方案是有原因的:Docker 在 `/host-project` 的 bind mount 是在容器创建时固定的，不重建容器就无法更改。但容器内 Linux 的 `/workspace` bind mount 可以被卸载并重新绑定到不同的子目录——某个 worktree——而无需影响容器生命周期。这就是 `coast assign` 很快的原因。

`/workspace` 是读写的。文件变更会立即双向同步。在主机上保存文件，Coast 内部的开发服务器会立刻拾取变更。在 Coast 内创建文件，它也会出现在主机上。

## 主机代理与 Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

由于文件系统是共享的，在主机上运行的 AI 编码代理可以自由编辑文件，而 Coast 内运行的服务会立即看到变更。代理不需要在 Coast 容器内运行——它像平常一样在主机上操作即可。

当代理需要运行时信息——日志、服务状态、测试输出——它会在主机上调用 Coast CLI 命令:

- `coast logs dev-1 --service web --tail 50` 查看服务输出（参见 [日志](LOGS.md)）
- `coast ps dev-1` 查看服务状态（参见 [运行时与服务](RUNTIMES_AND_SERVICES.md)）
- `coast exec dev-1 -- npm test` 在 Coast 内运行命令（参见 [Exec & Docker](EXEC_AND_DOCKER.md)）

这就是根本性的架构优势:**代码编辑发生在主机上，运行时发生在 Coast 中，共享文件系统把两者桥接起来。** 主机代理从不需要“进入” Coast 就能完成工作。

## Worktree 切换

当 `coast assign` 将某个 Coast 切换到不同的 worktree 时，它会重新挂载 `/workspace`，使其指向该 git worktree，而不是项目根目录:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

worktree 会在主机的 `{project_root}/.worktrees/{worktree_name}` 创建。`.worktrees` 目录名可通过 Coastfile 中的 `worktree_dir` 配置，并且应当加入你的 `.gitignore`。

如果 worktree 是新的，Coast 会在重新挂载之前从项目根目录引导（bootstrap）所选的 gitignored 文件。它使用 `git ls-files --others --ignored --exclude-standard` 枚举被忽略的文件，过滤掉常见的大型目录以及任何已配置的 `exclude_paths`，然后使用带 `--link-dest` 的 `rsync --files-from` 将选中的文件以硬链接方式导入到 worktree。Coast 会在内部 worktree 元数据中记录这次引导，并在之后对同一 worktree 的 assign 中跳过该步骤，除非你显式使用 `coast assign --force-sync` 刷新它。

在容器内，`/workspace` 会被 lazy-unmount，然后重新绑定到 `/host-project/.worktrees/{branch_name}` 下的 worktree 子目录。这个重新挂载很快——不会重建 DinD 容器，也不会重启内部 Docker daemon。在重新挂载之后，Compose 和裸服务仍可能被重建或重启，以便它们的 bind mount 能通过新的 `/workspace` 解析。

像 `node_modules` 这样的大型依赖目录不属于这条通用的引导路径。它们通常会通过服务特定的缓存或卷来处理。

如果你使用 `[assign.rebuild_triggers]`，Coast 还会在主机上运行 `git diff --name-only <previous>..<worktree>`，以决定标记为 `rebuild` 的服务是否可以降级为 `restart`。关于影响 assign 延迟的细节，参见 [Assign 与 Unassign](ASSIGN.md) 和 [性能优化](PERFORMANCE_OPTIMIZATIONS.md)。

`coast unassign` 会将 `/workspace` 还原回 `/host-project`（项目根目录）。停止后执行 `coast start` 会根据实例是否已分配 worktree 来重新应用正确的挂载。

## 所有挂载

每个 Coast 容器都有这些挂载:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | 项目根目录或 worktree。assign 时可切换。 |
| `/host-project` | Docker bind mount | RW | 原始项目根目录。容器创建时固定。 |
| `/image-cache` | Docker bind mount | RO | 来自 `~/.coast/image-cache/` 的预拉取 OCI tarball。 |
| `/coast-artifact` | Docker bind mount | RO | 带有重写后的 compose 文件的构建产物。 |
| `/coast-override` | Docker bind mount | RO | 为[共享服务](SHARED_SERVICES.md)生成的 compose override。 |
| `/var/lib/docker` | Named volume | RW | 内部 Docker daemon 状态。跨容器移除持久化。 |

只读挂载属于基础设施——它们承载 Coast 生成的构建产物、缓存镜像以及 compose override。你会通过 `coast build` 和 Coastfile 间接与它们交互。读写挂载则是你的代码所在之处，也是内部 daemon 存储其状态的位置。

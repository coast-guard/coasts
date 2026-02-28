# 文件系统

你的主机以及每个 Coast 实例共享同一份项目文件。主机项目根目录会以 bind mount 的方式挂载到 DinD 容器内的 `/workspace`，因此主机上的编辑会立刻出现在 Coast 内，反之亦然。这使得运行在主机上的 agent 能够编辑代码，而 Coast 内的服务能够实时拾取这些更改。

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

当容器创建时，主机项目根目录会以读写方式挂载到 [DinD 容器](RUNTIMES_AND_SERVICES.md) 内的 `/host-project`。容器启动后，容器内执行 `mount --bind /host-project /workspace`，创建工作路径 `/workspace`，并启用共享的挂载传播（`mount --make-rshared`），因此把 `/workspace` 的子目录进行 bind mount 的内部 compose 服务能看到正确的内容。

这种两阶段方法是有原因的:`/host-project` 的 Docker bind mount 在容器创建时就固定了，不能在不重建容器的情况下更改。但容器内的 Linux bind mount（`/workspace`）可以被卸载并重新绑定到不同的子目录——某个 worktree——而无需触碰容器生命周期。这正是 `coast assign` 速度快的原因。

`/workspace` 是读写的。文件更改会立即双向流动。在主机上保存一个文件，Coast 内的开发服务器会立刻拾取更改。在 Coast 内创建一个文件，它也会出现在主机上。

## 主机 Agent 与 Coast

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

因为文件系统是共享的，运行在主机上的 AI 编码 agent 可以自由编辑文件，而 Coast 内运行中的服务会立即看到这些更改。该 agent 不需要在 Coast 容器内运行——它像平常一样在主机上工作即可。

当 agent 需要运行时信息——日志、服务状态、测试输出——它会从主机调用 Coast CLI 命令:

- `coast logs dev-1 --service web --tail 50` 查看服务输出（参见 [日志](LOGS.md)）
- `coast ps dev-1` 查看服务状态（参见 [运行时与服务](RUNTIMES_AND_SERVICES.md)）
- `coast exec dev-1 -- npm test` 在 Coast 内运行命令（参见 [Exec 与 Docker](EXEC_AND_DOCKER.md)）

这就是根本性的架构优势:**代码编辑发生在主机上，运行时发生在 Coast 中，共享文件系统将两者桥接起来。** 主机上的 agent 从不需要“进入” Coast 就能完成工作。

## Worktree 切换

当 `coast assign` 将某个 Coast 切换到不同的 worktree 时，它会重新挂载 `/workspace`，使其指向该 git worktree，而不是项目根目录:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

worktree 会在主机上创建于 `{project_root}/.worktrees/{worktree_name}`。`.worktrees` 目录名可通过 Coastfile 中的 `worktree_dir` 配置，并且应当加入你的 `.gitignore`。

在容器内，`/workspace` 会被延迟卸载（lazy-unmounted），并重新绑定到 `/host-project/.worktrees/{branch_name}` 下的 worktree 子目录。这个重新挂载非常快——不会重建 DinD 容器，也不会重启内部 Docker daemon。内部的 compose 服务会被重建，以便它们的 bind mount 能通过新的 `/workspace` 进行解析。

像 `node_modules` 这类被 gitignore 的文件，会通过带硬链接的 rsync 从项目根目录同步到 worktree，因此即使依赖树很大，初次设置也几乎是瞬间完成。

在 macOS 上，主机与 Docker VM 之间的文件 I/O 存在固有开销。Coast 会在 assign 和 unassign 期间运行 `git ls-files` 来对 worktree 进行 diff，在大型代码库中这会带来明显的延迟。如果你项目中的某些部分不需要在多次 assign 之间进行 diff（文档、测试夹具、脚本），你可以在 Coastfile 中使用 `exclude_paths` 将它们排除，以降低这部分开销。详情参见 [Assign 与 Unassign](ASSIGN.md)。

`coast unassign` 会将 `/workspace` 还原回 `/host-project`（项目根目录）。停止后再执行 `coast start` 会根据该实例是否分配了 worktree 来重新应用正确的挂载。

## 所有挂载

每个 Coast 容器都有这些挂载:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | 项目根目录或 worktree。assign 时可切换。 |
| `/host-project` | Docker bind mount | RW | 原始项目根目录。容器创建时固定。 |
| `/image-cache` | Docker bind mount | RO | 来自 `~/.coast/image-cache/` 的预拉取 OCI tarballs。 |
| `/coast-artifact` | Docker bind mount | RO | 带有重写 compose 文件的构建产物。 |
| `/coast-override` | Docker bind mount | RO | 为 [共享服务](SHARED_SERVICES.md) 生成的 compose overrides。 |
| `/var/lib/docker` | Named volume | RW | 内部 Docker daemon 状态。跨容器删除仍会保留。 |

只读挂载属于基础设施——它们承载 Coast 生成的构建产物、缓存镜像以及 compose 覆盖文件。你会通过 `coast build` 和 Coastfile 间接与它们交互。读写挂载则是你的代码所在之处，也是内部 daemon 存储其状态的位置。

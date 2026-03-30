# 私有路径

当多个 Coast 实例共享同一个项目根目录时，它们会共享相同的文件——以及相同的 inode。通常这正是目的所在:主机上的文件更改会立即出现在 Coast 内部，因为两边看到的是同一个文件系统。但某些工具会将每个进程的状态写入工作区，并假定其拥有独占访问权；当两个实例共享同一个挂载时，这种假设就会失效。

## 问题

以 Next.js 16 为例，它在开发服务器启动时会通过 `flock(fd, LOCK_EX)` 对 `.next/dev/lock` 获取独占锁。`flock` 是一种基于 inode 级别的内核机制——它不关心挂载命名空间、容器边界或绑定挂载路径。如果两个不同 Coast 容器中的两个进程都指向同一个 `.next/dev/lock` inode（因为它们共享同一个主机绑定挂载），第二个进程就会看到第一个进程持有的锁，并拒绝启动:

```text
⨯ Another next dev server is already running.

- Local: http://localhost:3000
- PID: 1361
- Dir: /workspace/frontend
```

同类冲突也适用于:

- `flock` / `fcntl` 建议锁（Next.js、Turbopack、Cargo、Gradle）
- PID 文件（许多守护进程会写入 PID 文件并在启动时检查它）
- 假定单写者访问的构建缓存（Webpack、Vite、esbuild）

挂载命名空间隔离（`unshare`）对此无能为力。挂载命名空间控制的是进程可以看到哪些挂载点，而 `flock` 作用于 inode 本身。两个进程即使通过不同的挂载路径看到同一个 inode，仍然会发生冲突。

## 解决方案

`private_paths` Coastfile 字段用于声明应当按实例隔离的、相对于工作区的目录。每个 Coast 实例都会为这些路径获得各自独立的绑定挂载，其后端是容器自身文件系统中的每实例目录。

```toml
[coast]
name = "my-app"
private_paths = ["frontend/.next"]
```

在 Coast 以共享传播方式挂载 `/workspace` 之后，它会为每个私有路径额外应用一次绑定挂载:

```text
mkdir -p /coast-private/frontend/.next /workspace/frontend/.next
mount --bind /coast-private/frontend/.next /workspace/frontend/.next
```

`/coast-private/` 位于 DinD 容器的可写层上——而不是位于共享的主机绑定挂载上——因此每个实例天然会获得不同的 inode。`dev-1` 中的锁文件与 `dev-2` 中的锁文件位于不同的 inode 上，冲突也就消失了。

## 工作原理

在 Coast 生命周期中，每当 `/workspace` 被挂载或重新挂载时，都会应用私有路径挂载:

1. **`coast run`** —— 在初始执行 `mount --bind /host-project /workspace && mount --make-rshared /workspace` 之后，会挂载私有路径。
2. **`coast start`** —— 在容器重启后重新应用工作区绑定挂载之后。
3. **`coast assign`** —— 在卸载并将 `/workspace` 重新绑定到某个 worktree 目录之后。
4. **`coast unassign`** —— 在将 `/workspace` 恢复为项目根目录之后。

私有目录会在 stop/start 周期之间持久保留（它们位于容器文件系统中，而不是共享挂载上）。在 `coast assign` 或 `coast unassign` 时，私有目录会被**清空**，这样开发服务器会根据正确分支的源文件重新编译，而不是继续提供来自先前分支的陈旧构建输出。在执行 `coast rm` 时，它们会随容器一起被销毁。

## 何时使用

当某个工具将每进程或每实例状态写入工作区目录，并在并发的 Coast 实例之间发生冲突时，请使用 `private_paths`:

- **文件锁**:`.next/dev/lock`、Cargo 的 `target/.cargo-lock`、Gradle 的 `.gradle/lock`
- **构建缓存**:`.next`、`.turbo`、`target/`、`.vite`
- **PID 文件**:任何会将 PID 文件写入工作区的守护进程

不要将 `private_paths` 用于需要在实例之间共享或需要在主机上可见的数据。如果你需要持久化的、由 Docker 管理的隔离数据（例如数据库卷），请改用[使用 `strategy = "isolated"` 的 volumes](../coastfiles/VOLUMES.md)。

## 验证规则

- 路径必须是相对路径（不能以 `/` 开头）
- 路径中不得包含 `..` 组件
- 路径之间不得重叠——同时列出 `frontend/.next` 和 `frontend/.next/cache` 会报错，因为第一个挂载会遮蔽第二个

## 与 Volumes 的关系

`private_paths` 和 `[volumes]` 解决的是不同的隔离问题:

| | `private_paths` | `[volumes]` |
|---|---|---|
| **内容** | 相对于工作区的目录 | Docker 管理的命名卷 |
| **位置** | `/workspace` 内部 | 容器中的任意挂载路径 |
| **底层存储** | 容器本地文件系统（`/coast-private/`） | Docker 命名卷 |
| **隔离方式** | 始终按实例隔离 | `isolated` 或 `shared` 策略 |
| **在 `coast rm` 后是否保留** | 否 | Isolated:否。Shared:是。 |
| **使用场景** | 构建产物、锁文件、缓存 | 数据库、持久化应用数据 |

## 配置参考

完整语法和示例请参阅 Coastfile 参考中的 [`private_paths`](../coastfiles/PROJECT.md)。

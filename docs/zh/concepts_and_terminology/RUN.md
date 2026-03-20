# 运行

`coast run` 会创建一个新的 Coast 实例。它会解析最新的[构建](BUILDS.md)，配置一个[DinD 容器](RUNTIMES_AND_SERVICES.md)，加载缓存的镜像，启动你的 compose 服务，分配[动态端口](PORTS.md)，并将该实例记录到状态数据库中。

```bash
coast run dev-1
```

如果传入 `-w`，Coast 还会在配置完成后[分配](ASSIGN.md)该 worktree:

```bash
coast run dev-1 -w feature/oauth
```

这是最常见的模式:当某个 harness 或 agent 创建一个 worktree，并需要一步为它创建一个 Coast 时，就会使用这种方式。

## 会发生什么

`coast run` 执行四个阶段:

1. **验证并插入** —— 检查名称是否唯一，解析构建 ID（来自 `latest` 符号链接或显式指定的 `--build-id`），并插入一条 `Provisioning` 实例记录。
2. **Docker 配置** —— 在宿主守护进程上创建 DinD 容器，构建任何按实例生成的镜像，将缓存的镜像 tarball 加载到内部守护进程中，重写 compose 文件，注入密钥，并运行 `docker compose up -d`。
3. **完成** —— 存储端口分配，如果恰好只有一个端口则将其设为主端口，并将实例状态切换为 `Running`。
4. **可选的 worktree 分配** —— 如果提供了 `-w <worktree>`，则针对新实例运行 `coast assign`。如果分配失败，Coast 仍会继续运行——该失败会被记录为警告。

DinD 容器内持久化的 `/var/lib/docker` 卷意味着后续运行会跳过镜像加载。一次带冷缓存的全新 `coast run` 可能需要 20 秒以上；而在 `coast rm` 之后重新运行，通常会在 10 秒内完成。

## CLI 用法

```text
coast run <name> [options]
```

| 标志 | 说明 |
|------|-------------|
| `-w`, `--worktree <name>` | 在配置完成后分配此 worktree |
| `--n <count>` | 批量创建。名称中必须包含 `{n}`（例如 `coast run dev-{n} --n=5` 会创建 dev-1 到 dev-5） |
| `-t`, `--type <type>` | 使用类型化构建（例如 `--type snap` 会解析 `latest-snap` 而不是 `latest`） |
| `--force-remove-dangling` | 在创建前移除同名的残留 Docker 容器 |
| `-s`, `--silent` | 抑制进度输出；仅打印最终摘要或错误 |
| `-v`, `--verbose` | 显示详细信息，包括 Docker 构建日志 |

Git 分支始终会根据当前 HEAD 自动检测。

## 批量创建

在名称中使用 `{n}` 并配合 `--n`，即可一次创建多个实例:

```bash
coast run dev-{n} --n=5
```

这会按顺序创建 `dev-1`、`dev-2`、`dev-3`、`dev-4`、`dev-5`。每个实例都会获得各自独立的 DinD 容器、端口分配和卷状态。大于 10 的批量创建会提示确认。

## 类型化构建

如果你的项目使用多个 Coastfile 类型（参见 [Coastfile Types](COASTFILE_TYPES.md)），请传入 `--type` 以选择要使用的构建:

```bash
coast run dev-1                    # resolves "latest"
coast run test-1 --type test       # resolves "latest-test"
coast run snapshot-1 --type snap   # resolves "latest-snap"
```

## run、assign 和 remove 的区别

- `coast run` 会创建一个**新**实例。当你需要另一个 Coast 时使用它。
- `coast assign` 会将一个**现有**实例重新指向不同的 worktree。当你已经有一个 Coast，并且想切换它运行的代码时使用
  它。
- `coast rm` 会彻底拆除一个实例。当你想关闭
  Coast 或从零重新创建一个实例时使用它。

大多数日常切换并不需要 `coast rm`；通常 `coast assign` 和
`coast checkout` 就足够了。当你想进行一次干净的重新创建时，请使用 `coast rm`，
尤其是在你重建了 Coastfile 或构建之后。

你也可以将它们组合使用:`coast run dev-3 -w feature/billing` 会在一步中创建该实例
并分配该 worktree。

## 残留容器

如果之前的 `coast run` 被中断，或者 `coast rm` 未能完全清理，你可能会看到“残留 Docker 容器”错误。传入 `--force-remove-dangling` 以移除残留容器并继续:

```bash
coast run dev-1 --force-remove-dangling
```

## 另请参见

- [Remove](REMOVE.md) —— 彻底拆除一个实例
- [Builds](BUILDS.md) —— `coast run` 所使用的内容
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) —— 每个实例内部的 DinD 架构
- [Assign and Unassign](ASSIGN.md) —— 将现有实例切换到不同的 worktree
- [Ports](PORTS.md) —— 动态端口和规范端口如何分配
- [Coasts](COASTS.md) —— Coast 实例的高级概念

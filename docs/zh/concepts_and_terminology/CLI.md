# Coast CLI

Coast CLI（`coast`）是用于操作 Coasts 的主要命令行界面。它有意保持精简:解析你的命令，向 [`coastd`](DAEMON.md) 发送请求，并将结构化输出打印回你的终端。

## 你用它来做什么

典型工作流都由 CLI 驱动:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Coasts
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

CLI 还包含对人类和代理（agents）有用的文档命令:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## 为什么它与守护进程分开存在

将 CLI 与守护进程分离会带来一些重要收益:

- 守护进程负责保存状态并运行长生命周期进程。
- CLI 保持快速、可组合，并且易于编写脚本。
- 你可以在不保持终端状态存活的情况下运行一次性命令。
- 代理工具可以以可预测、对自动化友好的方式调用 CLI 命令。

## CLI vs Coastguard

使用哪个界面取决于当下需求:

- CLI 被设计用于完整的操作覆盖:你在 Coastguard 中能做的任何事，也都应该能从 CLI 完成。
- 将 CLI 视为自动化接口——脚本、代理工作流、CI 作业以及自定义开发者工具。
- 将 [Coastguard](COASTGUARD.md) 视为人类界面——可视化检查、交互式调试与运维可见性。

两者都与同一个守护进程通信，因此它们操作的是同一份底层项目状态。

# Coast CLI

Coast CLI（`coast`）是用于操作 Coast 的主要命令行界面。它被有意设计得很轻量:它会解析你的命令，将请求发送给 [`coastd`](DAEMON.md)，并将结构化输出打印回你的终端。

## 其用途

典型工作流都通过 CLI 驱动:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Run
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

CLI 还包含对人类和代理都很有用的文档命令:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## 为什么它与守护进程分离存在

将 CLI 与守护进程分离会带来一些重要好处:

- 守护进程负责保存状态和长生命周期进程。
- CLI 保持快速、可组合且易于编写脚本。
- 你可以运行一次性命令，而无需保持终端状态存活。
- 代理工具可以以可预测、适合自动化的方式调用 CLI 命令。

## CLI 与 Coastguard

使用最适合当前场景的界面:

- CLI 旨在提供完整的操作覆盖:你在 Coastguard 中能做的任何事情，也都应该可以通过 CLI 完成。
- 将 CLI 视为自动化接口——脚本、代理工作流、CI 作业以及自定义开发者工具。
- 将 [Coastguard](COASTGUARD.md) 视为人类界面——可视化检查、交互式调试和运行可见性。

两者都与同一个守护进程通信，因此它们基于相同的底层项目状态运行。

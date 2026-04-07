# 远程 Coast

> **测试版。** 远程 coast 已完全可用，但 CLI 标志、Coastfile schema 以及 coast-service API 可能会在未来版本中发生变化。如果你发现了 bug 或缺陷，请提交 pull request 或创建 issue。

远程 coast 会在远程机器上运行你的服务，同时保持与本地 coast 完全一致的开发体验。`coast run`、`coast assign`、`coast exec`、`coast ps`、`coast logs` 以及所有其他命令的工作方式都相同。守护进程会检测实例是否为远程实例，并通过 SSH 隧道透明地路由操作。

## 为什么使用远程

本地 coast 会在你的笔记本电脑上运行所有内容。每个 coast 实例都会运行一个完整的 Docker-in-Docker 容器，其中包含你的整个 compose 栈:Web 服务器、API、worker、数据库、缓存、邮件服务器。这样通常可行，直到你的笔记本电脑耗尽 RAM 或磁盘空间。

一个包含多个服务的全栈项目，每个 coast 可能都会消耗大量 RAM。并行运行几个 coast 后，你就会触及笔记本电脑的上限。

```text
  coast-1         coast-2         coast-3         coast-4
  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ worker   │   │ worker   │   │ worker   │   │ worker   │
  │ api      │   │ api      │   │ api      │   │ api      │
  │ admin    │   │ admin    │   │ admin    │   │ admin    │
  │ web      │   │ web      │   │ web      │   │ web      │
  │ mailhog  │   │ mailhog  │   │ mailhog  │   │ mailhog  │
  │          │   │          │   │          │   │          │
  │ 12 GB    │   │ 12 GB    │   │ 12 GB    │   │ 12 GB    │
  └──────────┘   └──────────┘   └──────────┘   └──────────┘

  Total: 48 GB RAM on your laptop
```

远程 coast 允许你通过将部分 coast 移动到远程机器上来进行水平扩展。DinD 容器、compose 服务和镜像构建会在远程运行，而你的编辑器和代理仍保留在本地。像 Postgres 和 Redis 这样的共享服务也保留在本地，并通过 SSH 反向隧道让你的数据库在本地与远程实例之间保持同步。

```text
  Your Machine                         Remote Server
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agents    │             │  coast-1 (all services) │
  │                     │  SSH        │  coast-2 (all services) │
  │  shared services    │──tunnels──▶ │  coast-3 (all services) │
  │  (postgres, redis)  │             │  coast-4 (all services) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: lightweight                  Server: 64 GB RAM, 16 CPU
```

对你的 localhost 运行时进行水平扩展。

## 快速开始

```bash
# 1. Register a remote machine
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote test my-vm

# 2. Build on the remote (uses remote's native architecture)
coast build --type remote

# 3. Run a remote coast
coast run dev-1 --type remote

# 4. Everything works as usual
coast ps dev-1
coast exec dev-1 -- bash
coast assign dev-1 --worktree feature/x
coast checkout dev-1
```

有关完整的设置说明，包括主机准备和 coast-service 部署，请参阅 [Setup](SETUP.md)。

## 参考

| 页面 | 内容说明 |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | 双容器拆分（shell coast + remote coast）、SSH 隧道层、端口转发链，以及守护进程如何路由请求 |
| [Setup](SETUP.md) | 主机要求、coast-service 部署、注册远程主机，以及端到端快速开始 |
| [File Sync](FILE_SYNC.md) | 用于批量传输的 rsync、用于持续同步的 mutagen、在 run/assign/stop 生命周期中的行为、排除规则，以及竞态条件处理 |
| [Builds](BUILDS.md) | 在远程端按原生架构构建、制品传输、`latest-remote` 符号链接、架构复用，以及自动清理 |
| [CLI and Configuration](CLI.md) | `coast remote` 命令、`Coastfile.remote` 配置、磁盘管理，以及 `coast remote prune` |

## 另请参阅

- [Remotes](../concepts_and_terminology/REMOTES.md) -- 术语表中的概念概览
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- 本地共享服务如何通过反向隧道连接到远程 coast
- [Ports](../concepts_and_terminology/PORTS.md) -- SSH 隧道层如何融入 canonical/dynamic 端口模型

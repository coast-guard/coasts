# Coastguard

Coastguard 是 Coast 的本地 Web UI（可理解为:Coast 的 Docker Desktop 风格界面），运行在端口 `31415`。它从 CLI 启动:

```bash
coast ui
```

![Coastguard project overview](../../assets/coastguard-overview.png)
*项目仪表板，显示正在运行的 Coast 实例、它们的分支/工作树，以及检出状态。*

![Coastguard port mappings](../../assets/coastguard-ports.png)
*特定 Coast 实例的端口页面，显示每个服务的规范端口与动态端口映射。*

## What Coastguard Is Good For

Coastguard 为你的项目提供可视化的控制与可观测性界面:

- 查看项目、实例、状态、分支，以及检出状态。
- 检查[端口映射](PORTS.md)并直接跳转到服务。
- 查看[日志](LOGS.md)、运行时统计，并检查数据。
- 浏览[构建](BUILDS.md)、镜像制品、[卷](VOLUMES.md)以及[密钥](SECRETS.md)元数据。
- 在工作时于应用内浏览文档。

## Relationship to CLI and Daemon

Coastguard 不会取代 CLI。它作为面向人的界面对 CLI 进行补充。

- [`coast` CLI](CLI.md) 是用于脚本、代理工作流以及工具集成的自动化接口。
- Coastguard 是用于可视化检查、交互式调试与日常运维可见性的面向人的界面。
- 二者都是 [`coastd`](DAEMON.md) 的客户端，因此始终保持同步。

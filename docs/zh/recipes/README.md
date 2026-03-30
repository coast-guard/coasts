# Recipes

Recipes 是针对常见项目形态的完整、带注释的 Coastfile 示例。每个 recipe 都包含一个完整的 Coastfile，你可以复制并进行调整，随后是按章节拆解的讲解，说明为何做出每个决策。

如果你是 Coastfile 新手，请先从 [Coastfiles reference](../coastfiles/README.md) 开始。Recipes 假设你熟悉核心概念。

- [Full-Stack Monorepo](FULLSTACK_MONOREPO.md) - 在宿主机上共享 Postgres 和 Redis、裸服务（bare-service）的 Vite 前端，以及通过 compose 进行容器化的后端。涵盖卷策略、健康检查（healthchecks）、assign 调优，以及用于大型仓库的 `exclude_paths`。
- [Next.js Application](NEXTJS.md) - 使用 Turbopack 的 Next.js、共享的 Postgres 和 Redis、后台工作进程，以及用于认证回调的动态端口处理。涵盖用于隔离 `.next` 的 `private_paths`、裸服务优化，以及多代理 worktree 支持。

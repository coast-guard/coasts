# 共享服务组

共享服务组（SSG）是一个 Docker-in-Docker 容器，用于在一个位置运行你项目的基础设施服务——Postgres、Redis、MongoDB，以及任何你原本会放在 `[shared_services]` 下的内容——并且它与使用这些服务的 [Coast](../concepts_and_terminology/COASTS.md) 实例分离。每个 Coast 项目都会有自己专属的 SSG，命名为 `<project>-ssg`，由项目 `Coastfile` 的同级文件 `Coastfile.shared_service_groups` 声明。

每个消费实例（`dev-1`、`dev-2`、...）都通过稳定的虚拟端口连接到其项目的 SSG，因此 SSG 重建不会影响消费者。每个 Coast 内部的契约保持不变:`postgres:5432` 会解析到你的共享 Postgres，应用代码不会知道这里有什么特殊之处。

## 为什么要使用 SSG

原始的 [共享服务](../concepts_and_terminology/SHARED_SERVICES.md) 模式会在宿主 Docker 守护进程上启动一个基础设施容器，并在项目中的每个消费实例之间共享它。对于单个项目来说，这样做完全没问题。问题出现在你有**两个不同的项目**，并且它们都声明了一个运行在 `5432` 上的 Postgres:两个项目都会尝试绑定相同的宿主端口，而第二个会失败。

```text
Without an SSG (cross-project host-port collision):

Host Docker daemon
+-- cg-coasts-postgres            (project "cg" binds host :5432)
+-- filemap-coasts-postgres       (project "filemap" tries :5432 -- FAILS)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (siblings share fine)
```

SSG 通过将每个项目的基础设施提升到其自己的 DinD 中来解决这个问题。Postgres 仍然监听规范的 `:5432`——但这是在 SSG 内部，而不是在宿主机上。SSG 容器会发布到一个任意的动态宿主端口，并且由守护进程管理的虚拟端口 socat（位于 `42000-43000` 范围内）会将消费者流量桥接到该端口。两个项目都可以各自拥有一个运行在规范 5432 上的 Postgres，因为它们都不会绑定宿主的 5432:

```text
With an SSG (per project, no cross-project collision):

Host Docker daemon
+-- cg-ssg                        (project "cg" -- DinD)
|     +-- postgres                (inner :5432, host dyn 54201, vport 42000)
|     +-- redis                   (inner :6379, host dyn 54202, vport 42001)
+-- filemap-ssg                   (project "filemap" -- DinD, no collision)
|     +-- postgres                (inner :5432, host dyn 54250, vport 42002)
|     +-- redis                   (inner :6379, host dyn 54251, vport 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

每个项目的 SSG 都拥有自己的数据、自己的镜像版本以及自己的密钥。两者永远不会共享状态，不会争抢端口，也看不到彼此的数据。在每个消费者 Coast 内部，契约保持不变:应用代码连接 `postgres:5432` 并获得它自己项目的 Postgres——其余工作由路由层完成（见 [路由](ROUTING.md)）。

## 快速开始

`Coastfile.shared_service_groups` 是项目 `Coastfile` 的同级文件。项目名称来自常规 Coastfile 中的 `[coast].name`——你不需要重复写它。

```toml
# Coastfile.shared_service_groups
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_DB = "app_dev" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

# Optional: extract secrets from your environment, keychain, or 1Password
# at build time and inject them into the SSG at run time. See SECRETS.md.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

构建并运行它:

```bash
coast ssg build       # parse, pull images, extract secrets, write artifact
coast ssg run         # start <project>-ssg, materialize secrets, compose up
coast ssg ps          # show service status
```

让一个消费者 Coast 指向它:

```toml
# Coastfile in the same project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

然后执行 `coast build && coast run dev-1`。如果 SSG 尚未运行，它会自动启动。在 `dev-1` 的应用容器内部，`postgres:5432` 会解析到 SSG 的 Postgres，且 `$DATABASE_URL` 会被设置为一个规范的连接字符串。

## 参考

| 页面 | 内容 |
|---|---|
| [Building](BUILDING.md) | `coast ssg build` 的端到端流程、每项目 artifact 布局、密钥提取、`Coastfile.shared_service_groups` 的发现规则，以及如何将项目锁定到特定构建 |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`、每项目 `<project>-ssg` 容器、在 `coast run` 时自动启动，以及用于跨项目列出的 `coast ssg ls` |
| [Routing](ROUTING.md) | 规范 / 动态 / 虚拟端口、宿主 socat 层、从应用到内部服务的完整逐跳链路，以及远程消费者的对称隧道 |
| [Volumes](VOLUMES.md) | 宿主绑定挂载、对称路径、内部命名卷、权限、`coast ssg doctor` 命令，以及如何将现有宿主卷迁移到 SSG 中 |
| [Consuming](CONSUMING.md) | `from_group = true`、允许和禁止的字段、冲突检测、`auto_create_db`、`inject`，以及远程消费者 |
| [Secrets](SECRETS.md) | SSG Coastfile 中的 `[secrets.<name>]`、构建时提取器流水线、通过 `compose.override.yml` 进行运行时注入，以及 `coast ssg secrets clear` 动词 |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout`，用于将 SSG 的规范端口绑定到宿主机上，以便宿主机上的任何工具（psql、redis-cli、IDE）都可以访问它们 |
| [CLI](CLI.md) | 每个 `coast ssg` 子命令的一行摘要 |

## 另请参阅

- [共享服务](../concepts_and_terminology/SHARED_SERVICES.md) -- SSG 所泛化的内联每实例模式
- [共享服务 Coastfile 参考](../coastfiles/SHARED_SERVICES.md) -- 包括 `from_group` 在内的消费者侧 TOML 语法
- [Coastfile: 共享服务组](../coastfiles/SHARED_SERVICE_GROUPS.md) -- `Coastfile.shared_service_groups` 的完整 schema
- [端口](../concepts_and_terminology/PORTS.md) -- 规范端口与动态端口

# Coastfile.shared_service_groups

`Coastfile.shared_service_groups` 是一个类型化的 Coastfile，用于声明你项目的 Shared Service Group (SSG) 将运行的服务。它与常规的 `Coastfile` 并列放置，项目名称来自该同级文件中的 `[coast].name` —— 你无需在这里重复。每个项目（在你的工作树中）恰好有一个这样的文件；`<project>-ssg` 容器会运行它所声明的服务。同一项目中的其他消费者 Coastfile 可以通过 `[shared_services.<name>] from_group = true` 引用这些服务。

关于该概念、生命周期、卷、密钥以及消费者接线方式，请参阅 [Shared Service Groups 文档](../shared_service_groups/README.md)。

## Discovery

`coast ssg build` 使用与 `coast build` 相同的规则来查找该文件:

- 默认:在当前工作目录中查找 `Coastfile.shared_service_groups` 或 `Coastfile.shared_service_groups.toml`。两种形式等价；如果两者都存在，则 `.toml` 变体优先。
- `-f <path>` / `--file <path>` 指向任意文件。
- `--working-dir <dir>` 将项目根目录与 Coastfile 位置解耦。
- `--config '<toml>'` 接受内联 TOML，用于脚本化流程。

## Accepted Sections

只接受 `[ssg]`、`[shared_services.<name>]`、`[secrets.<name>]` 和 `[unset]`。任何其他顶层键（`[coast]`、`[ports]`、`[services]`、`[volumes]`、`[assign]`、`[omit]`、`[inject]`、...）都会在解析时被拒绝。

支持使用 `[ssg] extends = "<path>"` 和 `[ssg] includes = ["<path>", ...]` 进行组合。参见下方的 [Inheritance](#inheritance)。

## `[ssg]`

顶层 SSG 配置。

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

外层 SSG DinD 的容器运行时。当前仅支持 `dind`；该字段是可选的，默认值为 `dind`。

## `[shared_services.<name>]`

每个服务对应一个块。TOML 键（`postgres`、`redis`、...）会成为消费者 Coastfile 引用的服务名。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

要在 SSG 的内部 Docker 守护进程中运行的 Docker 镜像。接受主机能够拉取的任何公共或私有镜像。

### `ports`

服务监听的容器端口。**仅允许裸整数。**

```toml
ports = [5432]
ports = [5432, 5433]
```

- `"HOST:CONTAINER"` 映射（`"5432:5432"`）会被**拒绝**。SSG 的主机端口发布始终是动态的 —— 你永远不会手动选择主机端口。
- 允许空数组（或完全省略该字段）。没有暴露端口的 sidecar 也是可以的。

在执行 `coast ssg run` 时，每个端口都会在外层 DinD 上变成一个 `PUBLISHED:CONTAINER` 映射，其中 `PUBLISHED` 是动态分配的主机端口。还会为每个项目分配一个独立的虚拟端口用于稳定的消费者路由 —— 参见 [Routing](../shared_service_groups/ROUTING.md)。

### `env`

扁平的字符串映射，会原样转发到内部服务容器的环境变量中。

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

环境变量的值**不会**被记录到构建清单中。只会记录键名，这与 `coast build` 的安全策略一致。

对于那些你不希望硬编码在 Coastfile 中的值（密码、API token），请使用下面介绍的 `[secrets.*]` 部分 —— 它会在构建时从主机提取，并在运行时注入。

### `volumes`

Docker-Compose 风格卷字符串的数组。每一项都是以下之一:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # 主机 bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # 内部命名卷
]
```

**主机 bind mount** —— 源以 `/` 开头。数据字节存储在真实主机文件系统上。外层 DinD 和内部服务都会绑定**相同的主机路径字符串**。参见 [Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan)。

**内部命名卷** —— 源是 Docker 卷名（不带 `/`）。该卷存在于 SSG 的内部 Docker 守护进程中。可在 SSG 重启后保持；对主机不透明。

在解析时会拒绝:

- 相对路径（`./data:/...`）。
- `..` 组件。
- 仅容器卷（无源）。
- 单个服务内重复的目标路径。

### `auto_create_db`

当为 `true` 时，守护进程会为每个运行的消费者 Coast 在此服务中创建一个 `{instance}_{project}` 数据库。仅适用于可识别的数据库镜像（Postgres、MySQL）。默认为 `false`。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

消费者 Coastfile 可以按项目覆盖该值 —— 参见 [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db)。

### `inject` (not allowed)

`inject` 在 SSG 服务定义中**无效**。注入是消费者侧的关注点（不同的消费者 Coastfile 可能希望将同一个 SSG Postgres 以不同的环境变量名暴露出来）。关于消费者侧的 `inject` 语义，参见 [Coastfile: Shared Services](SHARED_SERVICES.md#inject)。

## `[secrets.<name>]`

`Coastfile.shared_service_groups` 中的 `[secrets.*]` 块会在 `coast ssg build` 时提取主机侧凭证，并在 `coast ssg run` 时将其注入到 SSG 的内部服务中。该结构与常规 Coastfile 的 `[secrets.*]` 相同（字段参考请见 [Secrets](SECRETS.md)）；SSG 特定行为记录在 [SSG Secrets](../shared_service_groups/SECRETS.md) 中。

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"

[secrets.tls_cert]
extractor = "file"
path = "/Users/me/certs/dev.pem"
inject = "file:/etc/ssl/certs/server.pem"
```

可用的提取器相同（`env`、`file`、`command`、`keychain`、自定义 `coast-extractor-<name>`）。`inject` 指令用于选择该值是以环境变量还是文件形式进入 SSG 的内部服务容器。

默认情况下，SSG 原生密钥会注入到**每一个**已声明的 `[shared_services.*]`。如果只想作用于子集，请显式列出服务名:

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]      # 仅挂载到 postgres 服务
```

提取出的密钥值会以加密形式存储在 `~/.coast/keystore.db` 中，使用 `coast_image = "ssg:<project>"` —— 这是一个与常规 Coast keystore 条目分离的命名空间。完整生命周期（包括 `coast ssg secrets clear` 命令）请参见 [SSG Secrets](../shared_service_groups/SECRETS.md)。

## Inheritance

SSG Coastfile 支持与常规 Coastfile 相同的 `extends` / `includes` / `[unset]` 机制。共享的思维模型请参见 [Coastfile Inheritance](INHERITANCE.md)；本节记录 SSG 特有的形式。

### `[ssg] extends` -- 拉入父 Coastfile

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

父文件相对于子文件的父目录进行解析。适用 `.toml` 优先规则（解析器会先尝试 `Coastfile.ssg-base.toml`，再尝试普通的 `Coastfile.ssg-base`）。也接受绝对路径。

### `[ssg] includes` -- 合并片段文件

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

片段会按顺序在包含它们的文件本身之前合并。片段路径相对于包含文件的父目录解析（没有 `.toml` 优先规则 —— 片段通常按精确名称命名）。

**片段本身不能使用 `extends` 或 `includes`。** 它们必须是自包含的。

### Merge semantics

- **`[ssg]` 标量**（`runtime`）—— 子级存在时子级优先，否则继承。
- **`[shared_services.*]`** —— 按名称替换。如果父级和子级都定义了 `postgres`，则子级条目会完全替换父级条目（整条目替换，而不是字段级合并）。父级中未被子级重新声明的服务会被继承。
- **`[secrets.*]`** —— 按名称替换，形式与 `[shared_services.*]` 相同。具有相同名称的子级密钥会完全覆盖父级密钥配置。
- **加载顺序** —— 先加载 `extends` 父级，然后按顺序加载每个 `includes` 片段，最后加载顶层文件本身。后面的层在冲突时胜出。

### `[unset]` -- 删除继承的服务或密钥

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
secrets = ["pg_password"]
```

在合并**之后**移除命名条目，因此子级可以选择性地去掉父级提供的内容。支持 `shared_services` 和 `secrets` 这两个键。

独立的 SSG Coastfile 从技术上说也可以包含 `[unset]`，但它会被静默忽略（与常规 Coastfile 的行为一致:只有当文件参与继承时，unset 才会生效）。

### Cycles

直接循环（`A` extends `B` extends `A`，或 `A` extends 自己）会被硬错误处理，报错为 `circular extends/includes dependency detected: '<path>'`。菱形继承（两条不同路径最终都到达同一个父级）是允许的 —— 访问集合是按每次递归维护的，并会在返回时弹出。

### `[omit]` is not applicable

常规 Coastfile 支持 `[omit]` 用于从 compose 文件中移除服务 / 卷。SSG 没有可供裁剪的 compose 文件 —— 它直接从 `[shared_services.*]` 条目生成内部 compose。请改用 `[unset]` 来删除继承的服务。

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'` 无法解析父路径，因为没有磁盘上的位置可供相对路径锚定。在内联 TOML 中传递 `extends` / `includes` 会产生硬错误:`extends and includes require file-based parsing`。请改用 `-f <file>` 或 `--working-dir <dir>`。

### Build artifact is the flattened form

`coast ssg build` 会将一个独立 TOML 写入 `~/.coast/ssg/<project>/builds/<id>/ssg-coastfile.toml`。该产物包含继承处理后的合并结果，不含 `extends`、`includes` 或 `[unset]` 指令，因此即使父文件 / 片段文件不存在，也可以检查或重新运行该构建。`build_id` 哈希同样反映展平后的形式，因此仅父级发生变更时也会正确使缓存失效。

## Example

带有通过环境提取密码的 Postgres + Redis:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- 概念概览
- [SSG Building](../shared_service_groups/BUILDING.md) -- `coast ssg build` 如何处理此文件
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- 卷声明形式、权限以及主机卷迁移方案
- [SSG Secrets](../shared_service_groups/SECRETS.md) -- `[secrets.*]` 的构建时提取 / 运行时注入流程
- [SSG Routing](../shared_service_groups/ROUTING.md) -- 规范 / 动态 / 虚拟端口
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- 消费者侧的 `from_group = true` 语法
- [Coastfile: Secrets and Injection](SECRETS.md) -- 常规 Coastfile 的 `[secrets.*]` 参考
- [Coastfile Inheritance](INHERITANCE.md) -- 共享的 `extends` / `includes` / `[unset]` 思维模型

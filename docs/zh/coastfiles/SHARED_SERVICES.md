# 共享服务

`[shared_services.*]` 部分用于定义基础设施服务——数据库、缓存、消息代理——这些服务运行在宿主机 Docker 守护进程上，而不是在各个 Coast 容器内部运行。多个 Coast 实例通过桥接网络连接到同一个共享服务。

关于共享服务在运行时如何工作、生命周期管理以及故障排查，请参阅 [共享服务](../concepts_and_terminology/SHARED_SERVICES.md)。

## 定义一个共享服务

每个共享服务都是 `[shared_services]` 下一个具名的 TOML section。`image` 字段是必需的；其他所有字段都是可选的。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image`（必需）

要在宿主机守护进程上运行的 Docker 镜像。

### `ports`

服务暴露的端口列表。用于共享服务与 Coast 实例之间的桥接网络路由。

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

端口值必须为非零。

### `volumes`

用于持久化数据的 Docker volume 绑定字符串。这些是宿主机级别的 Docker volume，而不是由 Coast 管理的 volume。

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

传递给服务容器的环境变量。

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

当为 `true` 时，Coast 会在共享服务内为每个 Coast 实例自动创建一个按实例划分的数据库。默认为 `false`。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

将共享服务的连接信息以环境变量或文件的形式注入到 Coast 实例中。使用与 [secrets](SECRETS.md) 相同的 `env:NAME` 或 `file:/path` 格式。

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

## 生命周期

当第一个引用某个共享服务的 Coast 实例运行时，共享服务会自动启动。它们会在 `coast stop` 和 `coast rm` 之后继续运行——删除实例不会影响共享服务的数据。只有 `coast shared rm` 才会停止并移除共享服务。

由 `auto_create_db` 创建的按实例数据库也会在实例删除后保留。使用 `coast shared-services rm` 来移除该服务并彻底删除其数据。

## 何时使用共享服务 vs volumes

当多个 Coast 实例需要与同一个数据库服务器通信时（例如共享的 Postgres，每个实例拥有自己的数据库），请使用共享服务。当你希望控制 compose 内部服务的数据如何被共享或隔离时，请使用 [volume 策略](VOLUMES.md)。

## 示例

### Postgres、Redis 和 MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

### 最小化的共享 Postgres

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### 带有自动创建数据库的共享服务

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

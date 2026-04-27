# SSG 卷

在 `[shared_services.<name>]` 内部，`volumes` 数组使用标准的 Docker Compose 语法:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

以 `/` 开头表示一个**主机绑定路径**——字节存储在主机文件系统上，内部服务会直接就地读取和写入它们。如果不以斜杠开头，例如 `pg_wal:/var/lib/postgresql/wal`，则源是一个**位于 SSG 嵌套 Docker 守护进程中的 Docker 命名卷**——它会在 `coast ssg rm` 后保留，并在 `coast ssg rm --with-data` 时被删除。两种形式都接受。

在解析阶段会被拒绝的情况:相对路径（`./data:/...`）、`..` 组件、仅容器卷（没有源），以及同一服务内重复的目标路径。

## 重用来自 docker-compose 或内联共享服务的 Docker 卷

如果你的数据已经存在于主机 Docker 命名卷中——来自 `docker-compose up`、来自内联 `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]`，或者来自手动执行的 `docker volume create`——你可以通过绑定挂载该卷底层的主机目录，让 SSG 读取相同的字节:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

左侧是现有 Docker 卷在主机文件系统上的路径；`docker volume inspect <name>` 会在 `Mountpoint` 字段中报告它。Coast 不会复制字节——SSG 读取和写入的是 docker-compose 使用过的同一批文件。`coast ssg rm`（不带 `--with-data`）不会触碰该卷，因此 docker-compose 也可以继续使用它。

> **为什么不能直接写成 `infra_postgres_data:/var/lib/postgresql/data`？** 这对于内联 `[shared_services.*]` 是可行的（该卷会在主机 Docker 守护进程上创建，docker-compose 可以看到它）。但在 SSG 内部并不会以相同方式工作——没有前导斜杠的名称会在 SSG 的嵌套 Docker 守护进程中创建一个全新的卷，与主机隔离。当你想与任何运行在主机守护进程上的东西共享数据时，请改用该卷的挂载点路径。

### `coast ssg import-host-volume`

`coast ssg import-host-volume` 会通过 `docker volume inspect` 解析该卷的 `Mountpoint`，并输出（或应用）等效的 `volumes` 行，因此你不需要手动构造 `/var/lib/docker/volumes/<name>/_data` 路径。

片段模式（默认）会打印可粘贴的 TOML 片段:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

输出是一个 `[shared_services.postgres]` 块，其中已合并新的 `volumes = [...]` 条目:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

应用模式会原地重写 `Coastfile.shared_service_groups`，并将原始文件保存为 `Coastfile.shared_service_groups.bak`:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

标志:

- `<VOLUME>`（位置参数）——主机 Docker 命名卷。必须已经存在（检查方式是 `docker volume inspect`）；否则请先用 `docker volume create` 创建或重命名。
- `--service` ——要编辑的 `[shared_services.<name>]` 段。该段必须已经存在。
- `--mount` ——绝对容器路径。相对路径会被拒绝。同一服务上的重复挂载路径会被视为硬错误。
- `--file` / `--working-dir` / `--config` ——SSG Coastfile 发现方式，规则与 `coast ssg build` 相同。
- `--apply` ——原地重写 Coastfile。不能与 `--config` 一起使用（内联文本没有可回写的地方）。

`.bak` 文件会逐字节保留原始内容，因此你可以恢复到应用前的精确状态。

`/var/lib/docker/volumes/<name>/_data` 是 Docker 多年来一直用作卷挂载点的路径，也是 `docker volume inspect` 当前报告的内容。Docker 并未正式承诺会永远保留此路径；如果未来的 Docker 版本将卷移到别处，请重新运行 `coast ssg import-host-volume` 以获取新路径。

## 权限

当数据目录归属于错误的用户时，多个镜像会拒绝启动。常见的包括 Postgres（debian 标签中 UID 999，alpine 标签中 UID 70）、MySQL/MariaDB（UID 999）以及 MongoDB（UID 999）。如果主机目录归 root 所有，Postgres 会在启动时退出，并给出一句简短的 “data directory has wrong ownership”。

修复只需要一条命令:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

请在 `coast ssg run` 之前执行此操作。如果目录尚不存在，`coast ssg run` 会使用默认所有权创建它（Linux 上为 root，macOS 上通过 Docker Desktop 为你的用户）。对于 Postgres 来说，这个默认值通常是错误的。如果你是通过 `coast ssg import-host-volume` 进入的，并且 `docker-compose up` 之前已经在首次启动时对该卷执行过 `chown`，那你已经没问题了。

## `coast ssg doctor`

`coast ssg doctor` 是一个只读检查，会针对当前项目的 SSG 运行（从当前工作目录的 `Coastfile` 中的 `[coast].name` 或 `--working-dir` 解析得到）。它会为活动构建中的每个 `(service, host-bind)` 对输出一条发现结果，外加 secret 提取发现（参见 [Secrets](SECRETS.md)）。

对于每个已知镜像（Postgres、MySQL、MariaDB、MongoDB），它会查询内置的 UID/GID 表，与每个主机路径上的 `stat(2)` 结果进行比较，并输出:

- 当所有者与镜像预期一致时，输出 `ok`。
- 当不一致时，输出 `warn`。消息中包含用于修复的 `chown` 命令。
- 当目录尚不存在，或者匹配的镜像仅使用命名卷（主机侧没有可检查内容）时，输出 `info`。

镜像不在已知镜像表中的服务会被静默跳过。像 `ghcr.io/baosystems/postgis` 这样的分叉不会被标记——doctor 宁可什么也不说，也不愿发出错误警告。

```bash
coast ssg doctor
```

Postgres 目录所有权不匹配时的示例输出:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor 不会修改任何内容。你放在主机文件系统上的字节权限并不是 Coast 会静默更改的东西。

## 平台说明

- **macOS Docker Desktop。** 原始主机路径必须列在 Settings -> Resources -> File Sharing 下。默认包含 `/Users`、`/Volumes`、`/private`、`/tmp`。`/var/coast-data` 在 macOS 上**不**在默认列表中——对于新的路径，优先使用 `$HOME/coast-data/...`，或将 `/var/coast-data` 添加到 File Sharing。`/var/lib/docker/volumes/<name>/_data` 这种形式*不是*主机路径——Docker 会在它自己的虚拟机内解析它——因此无需 File Sharing 条目即可工作。
- **WSL2。** 优先使用 WSL 原生路径（`~`、`/mnt/wsl/...`）。`/mnt/c/...` 也能工作，但由于 9P 协议桥接 Windows 主机文件系统，速度较慢。
- **Linux。** 没有特殊问题。

## 生命周期

- `coast ssg rm` ——移除 SSG 的外层 DinD 容器。**卷内容不会被触碰**，主机绑定挂载内容不会被触碰，密钥库不会被触碰。任何其他使用同一个 Docker 卷的东西都能继续工作。
- `coast ssg rm --with-data` ——删除**位于 SSG 嵌套 Docker 守护进程内部**的卷（即没有前导斜杠的 `name:path` 形式）。主机绑定挂载和外部 Docker 卷仍然不会被触碰——Coast 不拥有它们。
- `coast ssg build` ——绝不会触碰卷。只会写入一个清单，以及（当声明了 `[secrets]` 时）密钥库条目。
- `coast ssg run` / `start` / `restart` ——如果主机绑定挂载目录不存在，则会创建它们（使用默认所有权——见 [权限](#权限)）。

## 另请参阅

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) ——包括卷语法在内的完整 TOML 架构
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) ——适用于非 SSG 服务的共享、隔离和快照播种卷策略
- [Building](BUILDING.md) ——清单的来源
- [Lifecycle](LIFECYCLE.md) ——卷何时被创建、停止和删除
- [Secrets](SECRETS.md) ——文件注入的 secrets 会落在 `~/.coast/ssg/runs/<project>/secrets/<basename>`，并以只读方式绑定挂载到内部服务中

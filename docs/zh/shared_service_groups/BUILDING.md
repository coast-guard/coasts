# 构建共享服务组

`coast ssg build` 会解析你项目的 `Coastfile.shared_service_groups`，提取所有声明的密钥，将每个镜像拉取到主机镜像缓存中，并在 `~/.coast/ssg/<project>/builds/<build_id>/` 下写入一个带版本的构建产物。该命令不会破坏已经运行中的 SSG——下一次 `coast ssg run` 或 `coast ssg start` 会拾取新的构建，但正在运行的 `<project>-ssg` 会继续提供其当前构建，直到你重启它。

项目名称来自同级 `Coastfile` 中的 `[coast].name`。每个项目都有自己名为 `<project>-ssg` 的 SSG、自己的构建目录，以及自己的 `latest_build_id`——不存在主机范围内的“当前 SSG”。

完整的 TOML 模式请参见 [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md)。

## 发现

`coast ssg build` 使用与 `coast build` 相同的规则来查找其 Coastfile:

- 在没有任何标志时，它会在当前工作目录中查找 `Coastfile.shared_service_groups` 或 `Coastfile.shared_service_groups.toml`。两种形式是等价的，当两者都存在时，`.toml` 后缀优先。
- `-f <path>` / `--file <path>` 指向任意文件。
- `--working-dir <dir>` 将项目根目录与 Coastfile 位置解耦（与 `coast build --working-dir` 相同的标志）。
- `--config '<inline-toml>'` 支持脚本和 CI 流程，你可以在其中内联生成 Coastfile。

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

构建会从同一目录中的同级 `Coastfile` 解析项目名称。如果你使用 `--config`（没有磁盘上的 Coastfile.shared_service_groups），当前工作目录仍必须包含一个 `Coastfile`，其 `[coast].name` 就是 SSG 项目。

## 构建会做什么

每次 `coast ssg build` 都会通过与 `coast build` 相同的 `BuildProgressEvent` 通道流式传输进度，因此 CLI 会渲染 `[N/M]` 步骤计数器。

1. **解析** `Coastfile.shared_service_groups`。`[ssg]`、`[shared_services.*]`、`[secrets.*]` 和 `[unset]` 是可接受的顶级节。卷条目会被拆分为主机绑定挂载和内部命名卷（参见 [Volumes](VOLUMES.md)）。
2. **解析构建 id。** 该 id 的形式为 `{coastfile_hash}_{YYYYMMDDHHMMSS}`。该哈希会纳入原始源码、已解析服务的确定性摘要，以及 `[secrets.*]` 配置（因此，编辑某个密钥的 `extractor` 或 `var` 会生成新的 id）。
3. **合成内部 `compose.yml`。** 每个 `[shared_services.*]` 块都会变成单个 Docker Compose 文件中的一个条目。这就是 SSG 的内部 Docker 守护进程在 `coast ssg run` 时通过 `docker compose up -d` 运行的文件。
4. **提取密钥。** 当 `[secrets.*]` 非空时，运行每个声明的提取器，并将加密结果以 `coast_image = "ssg:<project>"` 存储在 `~/.coast/keystore.db` 中。如果 Coastfile 没有 `[secrets]` 块，则会静默跳过。完整流程请参见 [Secrets](SECRETS.md)。
5. **拉取并缓存每个镜像。** 镜像会作为 OCI tarball 存储在 `~/.coast/image-cache/` 中，这是 `coast build` 使用的同一个池。任一命令的缓存命中都会加速另一个命令。
6. **写入构建产物** 到 `~/.coast/ssg/<project>/builds/<build_id>/`，包含三个文件:`manifest.json`、`ssg-coastfile.toml` 和 `compose.yml`（见下方布局）。
7. **更新项目的 `latest_build_id`。** 这是一个状态数据库标志，而不是文件系统符号链接。`coast ssg run` 和 `coast ssg ps` 会读取它以确定要操作哪个构建。
8. **自动修剪** 此项目较旧的构建，仅保留最近的 5 个。`~/.coast/ssg/<project>/builds/` 下更早的产物目录会从磁盘中删除。被固定的构建（见下方“将项目锁定到特定构建”）始终会被保留。

## 产物布局

```text
~/.coast/
  keystore.db                                          （共享，由 coast_image 命名空间隔离）
  keystore.key
  image-cache/                                         （共享的 OCI tarball 池）
  ssg/
    cg/                                                （项目 "cg"）
      builds/
        b455787d95cfdeb_20260420061903/                （新构建）
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               （先前的构建）
          ...
    filemap/                                           （项目 "filemap"——独立树）
      builds/
        ...
    runs/
      cg/                                              （按项目划分的运行临时区）
        compose.override.yml                           （在 coast ssg run 时渲染）
        secrets/<basename>                             （文件注入的密钥，模式 0600）
```

`manifest.json` 记录了下游代码所关心的构建元数据:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

其中有意不包含环境变量值和密钥负载——只记录环境变量名和注入 *目标*。密钥值以加密形式存储在 keystore 中，绝不会出现在产物文件里。

`ssg-coastfile.toml` 是已解析、已插值、已通过校验后的 Coastfile。它在字节级别上与守护进程在解析时看到的内容完全一致。适合用于审计过去的构建。

`compose.yml` 是 SSG 的内部 Docker 守护进程运行的内容。有关合成规则，尤其是对称路径 bind mount 策略，请参见 [Volumes](VOLUMES.md)。

## 在不运行构建的情况下检查它

`coast ssg ps` 会直接读取该项目 `latest_build_id` 对应的 `manifest.json`——它不会检查任何容器。你可以在 `coast ssg build` 之后立即运行它，以查看下一次 `coast ssg run` 将启动的服务:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

`PORT` 列是内部容器端口。动态主机端口会在 `coast ssg run` 时分配；面向消费者的虚拟端口由 `coast ssg ports` 报告。完整说明请参见 [Routing](ROUTING.md)。

要浏览某个项目的所有构建（包括时间戳、服务数量，以及当前哪个构建是 latest），请使用:

```bash
coast ssg builds-ls
```

## 重新构建

新的 `coast ssg build` 是更新 SSG 的规范方式。它会重新提取密钥（如果有）、更新 `latest_build_id`，并修剪旧产物。消费者不会自动重新构建——它们的 `from_group = true` 引用会在消费者构建时，针对当时的当前构建进行解析。要将消费者切换到较新的 SSG，请为该消费者运行 `coast build`。

运行时在跨重建场景下是宽容的:虚拟端口会对 `(project, service, container_port)` 保持稳定，因此消费者不需要为了路由而刷新。形状变化（某个服务被重命名或移除）会在消费者层表现为连接错误，而不是 Coast 层面的“漂移”消息。原因请参见 [Routing](ROUTING.md)。

## 将项目锁定到特定构建

默认情况下，SSG 运行该项目的 `latest_build_id`。如果你需要将项目冻结在更早的某个构建上——用于回归复现、跨 worktree 对比两个构建的 A/B，或让长期存在的分支保持在已知良好的形态上——请使用 pin 命令:

```bash
coast ssg checkout-build <build_id>     # 将此项目固定到 <build_id>
coast ssg show-pin                      # 报告当前活动的固定（如果有）
coast ssg uncheckout-build              # 释放固定；回到 latest
```

固定是按消费者项目划分的（每个项目一个固定，在各个 worktree 之间共享）。当被固定时:

- `coast ssg run` 会自动启动被固定的构建，而不是 `latest_build_id`。
- `coast build` 会根据被固定构建的 manifest 校验 `from_group` 引用。
- `auto_prune` 不会删除被固定的构建目录，即使它落在最近 5 个之外。

当固定处于活动状态时，Coastguard SPA 会在构建 id 旁显示 `PINNED` 标记；未固定时显示 `LATEST`。pin 命令也出现在 [CLI](CLI.md) 中。

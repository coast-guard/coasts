# 端口

`[ports]` 部分声明 Coast 管理哪些端口，用于在你的 Coast 实例与宿主机之间进行转发。可选的 `[egress]` 部分声明宿主机上的端口，这些端口是 Coast 实例需要向外访问的。

关于端口转发在运行时如何工作——规范端口 vs 动态端口、checkout 切换、socat——参见 [Ports](../concepts_and_terminology/PORTS.md) 和 [Checkout](../concepts_and_terminology/CHECKOUT.md)。

## `[ports]`

一个扁平映射:`logical_name = port_number`。每个条目告诉 Coast 在某个 Coast 实例运行时为该端口设置端口转发。

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

每个实例都会为每个已声明的端口获得一个动态端口（高位范围，始终可访问）。被 [checked-out](../concepts_and_terminology/CHECKOUT.md) 的实例还会将规范端口（你声明的数字）转发到宿主机。

规则:

- 端口值必须是非零的无符号 16 位整数（1-65535）。
- 逻辑名称是自由格式字符串，用作 `coast ports`、Coastguard 和 `primary_port` 中的标识符。

### 最小示例

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 3000
```

### 多服务示例

```toml
[ports]
web = 3000
api = 4000
backend = 8080
postgres = 5432
redis = 6379
```

## `primary_port`

在 `[coast]` 部分中设置（在 [Project and Setup](PROJECT.md) 中有文档），`primary_port` 为你已声明的端口之一命名，用于在 [Coastguard](../concepts_and_terminology/COASTGUARD.md) 中的快速链接与子域名路由。

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

该值必须匹配 `[ports]` 中的一个键。详情参见 [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md)。

## `[egress]`

声明宿主机上 Coast 实例需要访问的端口。这与 `[ports]` 的方向相反——`[ports]` 是将端口从 Coast *转发到* 宿主机，而 egress 则让宿主机端口可以在 Coast *内部被访问到*。

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 48090

[egress]
host-api = 48080
```

当你在 Coast 内部的 compose 服务需要与直接运行在宿主机上的某些东西（在 Coast 的共享服务系统之外）通信时，这会很有用。

规则:

- 与 `[ports]` 相同:值必须是非零的无符号 16 位整数。
- 逻辑名称是自由格式的标识符。

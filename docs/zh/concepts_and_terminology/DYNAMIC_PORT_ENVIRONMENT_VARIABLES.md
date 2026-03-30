# 动态端口环境变量

每个 Coast 实例都会获得一组环境变量，用于暴露分配给各个服务的[动态端口](PORTS.md)。这些变量在裸服务和 compose 容器内部都可用，使你的应用能够在运行时发现其可从外部访问的端口。

## 命名约定

Coast 根据你在 `[ports]` 部分中的逻辑服务名派生变量名:

1. 转换为大写
2. 将非字母数字字符替换为下划线
3. 追加 `_DYNAMIC_PORT`

```text
[ports] key          Environment variable
─────────────        ────────────────────────────
web             →    WEB_DYNAMIC_PORT
postgres        →    POSTGRES_DYNAMIC_PORT
backend-test    →    BACKEND_TEST_DYNAMIC_PORT
svc.v2          →    SVC_V2_DYNAMIC_PORT
```

如果服务名以数字开头，Coast 会在变量名前加上下划线（例如，`9svc` 会变成 `_9SVC_DYNAMIC_PORT`）。空名称则回退为 `SERVICE_DYNAMIC_PORT`。

## 示例

给定这个 Coastfile:

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

从这个构建创建的每个 Coast 实例都会有三个额外的环境变量:

```text
WEB_DYNAMIC_PORT=62217
API_DYNAMIC_PORT=55681
POSTGRES_DYNAMIC_PORT=56905
```

实际的端口号会在 `coast run` 时分配，并且每个实例都不同。

## 何时使用它们

最常见的使用场景是配置那些会在响应中嵌入自身 URL 的服务:身份验证回调、OAuth 重定向 URI、CORS 源，或 webhook URL。这些服务需要知道外部客户端使用的端口，而不是它们内部监听的端口。

例如，使用 NextAuth 的 Next.js 应用需要将 `AUTH_URL` 设置为可从外部访问的地址。在 Coast 内部，Next.js 始终监听 3000 端口，但主机侧端口是动态的:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} yarn dev:web"
port = 3000
```

`:-3000` 回退意味着该命令在 Coast 外部也能工作，因为此时 `WEB_DYNAMIC_PORT` 未设置。

## 优先级

如果 Coast 容器中已经存在同名环境变量（通过 secrets、inject 或 compose environment 设置），Coast 不会覆盖它。现有值具有更高优先级。

## 可用性

动态端口变量会在启动时注入到 Coast 容器的环境中。它们可用于:

- 裸服务的 `install` 命令
- 裸服务的 `command` 进程
- Compose 服务容器（通过容器环境）
- 通过 `coast exec` 运行的命令

这些值在实例的整个生命周期内都不会改变。如果你停止并重新启动实例，它会保留相同的动态端口。

## 另请参阅

- [Ports](PORTS.md) - 规范端口与动态端口，以及 checkout 如何在它们之间切换
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - 实例之间的子域路由和 cookie 隔离

# 主端口与 DNS

主端口是一个可选的便捷功能，用于为你的某个服务创建快速链接——通常是你的 Web 前端。它会在 Coastguard 中显示为一个可点击的徽章，并在 `coast ports` 中显示为带星标的条目。它不会改变端口的工作方式；只是选定一个用于高亮显示。

## 设置主端口

在 Coastfile 的 `[coast]` 部分添加 `primary_port`，并引用 [`[ports]`](PORTS.md) 中的一个键:

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
backend = 8080
```

如果你的项目只有一个端口，Coast 会自动将其检测为主端口——你无需显式设置。

你也可以在 Coastguard 的 Ports 标签页中，通过点击任意服务旁的星标图标来切换主端口，或在 CLI 中使用 `coast ports set-primary`。该设置是按构建（per-build）生效的，因此由同一构建创建的所有实例共享同一个主端口。

## 它启用的内容

```text
coast ports dev-1

  SERVICE    CANONICAL  DYNAMIC
  ★ web      3000       62217
    backend  8080       63889
```

带星标的服务就是你的主端口。在 Coastguard 中，它会作为实例名称旁的可点击徽章出现——点击一次即可在浏览器中打开你的应用。

这对以下场景特别有用:

- **主机侧代理（Host-side agents）** —— 给你的 AI 代理一个用于对照检查变更的单一 URL。你无需告诉它“打开 localhost:62217”，主端口 URL 可从 `coast ls` 和守护进程 API 以编程方式获取。
- **浏览器 MCP** —— 如果你的代理使用浏览器 MCP 来验证 UI 变更，主端口 URL 是指向它的规范目标（canonical target）。
- **快速迭代** —— 一键访问你最常查看的服务。

主端口完全是可选的。不设置也不会影响任何功能——它只是用于更快导航的体验优化功能。

## 子域路由

当你运行多个带隔离数据库的 Coast 实例时，它们在浏览器中都共享 `localhost`。这意味着由 `localhost:62217`（dev-1）设置的 cookie 对 `localhost:63104`（dev-2）也是可见的。如果你的应用使用会话 cookie，登录其中一个实例可能会干扰另一个。

子域路由通过为每个实例提供自己的 origin 来解决这个问题:

```text
Without subdomain routing:
  dev-1 web  →  http://localhost:62217
  dev-2 web  →  http://localhost:63104
  (cookies shared — both are "localhost")

With subdomain routing:
  dev-1 web  →  http://dev-1.localhost:62217
  dev-2 web  →  http://dev-2.localhost:63104
  (cookies isolated — different subdomains)
```

你可以在每个项目中通过 Coastguard 的 Ports 标签页启用它（页面底部的开关），或通过守护进程 settings API 启用。

### 权衡:CORS

缺点是你的应用可能需要调整 CORS。如果你的前端位于 `dev-1.localhost:3000`，并向 `dev-1.localhost:8080` 发起 API 请求，浏览器会将其视为跨源（cross-origin），因为端口不同。大多数开发服务器已能处理这一点，但如果你在启用子域路由后看到 CORS 错误，请检查你应用的允许来源（allowed origins）配置。

## URL 模板

每个服务都有一个 URL 模板，用于控制其链接如何生成。默认值为:

```text
http://localhost:<port>
```

`<port>` 占位符会替换为实际端口号——当实例被[检出](CHECKOUT.md)时使用规范端口（canonical port），否则使用动态端口。当启用子域路由时，`localhost:` 会替换为 `{instance}.localhost:`。

你可以在 Coastguard 的 Ports 标签页为每个服务自定义模板（每个服务旁的铅笔图标）。如果你的开发服务器使用 HTTPS、自定义主机名或非标准 URL scheme，这会很有用:

```text
https://my-service.localhost:<port>
```

模板存储在守护进程设置中，并会在重启后保持。

## DNS 设置

大多数浏览器默认会将 `*.localhost` 解析到 `127.0.0.1`，因此子域路由无需任何 DNS 配置即可工作。

如果你需要自定义域名解析（例如 `*.localcoast`），Coast 包含一个内嵌 DNS 服务器。一次性设置即可:

```bash
coast dns setup    # writes /etc/resolver/localcoast (requires sudo)
coast dns status   # check if DNS is configured
coast dns remove   # remove the resolver entry
```

这是可选的，只有在你的浏览器中 `*.localhost` 无法工作或你希望使用自定义 TLD 时才需要。

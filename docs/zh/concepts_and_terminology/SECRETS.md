# Secrets 和 Extractors

Secrets 是从你的主机机器提取并以环境变量或文件的形式注入到 Coast 容器中的值。Coast 会在构建时提取 secrets，将其在本地密钥库中静态加密存储，并在创建 Coast 实例时注入它们。

## 注入类型

每个 secret 都有一个 `inject` 目标，用于控制它如何被传递到 Coast 容器中:

- `env:VAR_NAME` — 作为环境变量注入。
- `file:/path/in/container` — 作为容器内的文件挂载。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.credentials]
extractor = "file"
path = "~/.config/my-app/credentials.json"
inject = "file:/run/secrets/credentials.json"
```

## 内置 Extractors

### env

读取主机环境变量。这是最常见也是最简单的 extractor。如果你的主机上已经有以环境变量形式存在的 secrets——来自 `.env` 文件、`direnv`、shell 配置文件或任何其他来源——只需将它们转发到 Coast 中即可。

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DATABASE_PASSWORD"
```

大多数项目仅使用 `env` extractor 就足够了。

### file

从主机文件系统读取文件。支持对主目录路径进行 `~` 展开。适用于 SSH 密钥、TLS 证书以及凭据 JSON 文件。

```toml
[secrets.ssh_key]
extractor = "file"
path = "~/.ssh/id_ed25519"
inject = "file:/run/secrets/ssh_key"
```

### command

运行 shell 命令并将 stdout 捕获为 secret 值。命令通过 `sh -c` 执行，因此管道、重定向以及变量展开都可用。这对于从 1Password CLI、HashiCorp Vault 或任何动态来源拉取 secrets 很有用。

```toml
[secrets.op_token]
extractor = "command"
run = "op read 'op://vault/db/password'"
inject = "env:DATABASE_PASSWORD"
```

你也可以使用 `command` 来转换或从本地配置文件中提取特定字段:

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; print(json.dumps({\"key\": \"value\"}))"'
inject = "file:/root/.claude.json"
```

### keychain

`macos-keychain` 的别名。从 macOS 钥匙串读取通用密码项目。

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

keychain extractor 通常并非必需。如果你可以通过环境变量或文件获得相同的值，优先选择这些更简单的方法。当 secret 仅存在于 macOS 钥匙串中且不易导出时，钥匙串提取会很有用——例如，第三方工具写入到钥匙串中的、由应用程序专用的凭据。

`account` 参数是可选的，默认使用你的 macOS 用户名。

此 extractor 仅在 macOS 上可用。在其他平台引用它会在构建时产生清晰的错误。

## 自定义 Extractors

如果内置 extractors 都不符合你的工作流，Coast 会回退为在你的 PATH 中查找名为 `coast-extractor-{name}` 的可执行文件。该可执行文件会从 stdin 接收以 JSON 表示的 extractor 参数，并应将 secret 值写入 stdout。

```toml
[secrets.vault_token]
extractor = "vault"
path = "secret/data/token"
inject = "env:VAULT_TOKEN"
```

Coast 会调用 `coast-extractor-vault`，并在 stdin 上传入 `{"path": "secret/data/token"}`。退出码 0 表示成功；非 0 表示失败（stderr 会包含在错误消息中）。

## 非 secret 注入

`[inject]` 部分将主机环境变量和文件转发到 Coast，而不将其视为 secrets。这些值不会被加密——它们会被直接传递。

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.gitconfig", "~/.npmrc"]
```

对不敏感的配置使用 `[inject]`。对任何需要静态加密的内容使用 `[secrets]`。

## Secrets 不会存储在构建产物中

Secrets 会在构建时被提取，但不会被烘焙进 coast 构建产物中。它们会在使用 `coast run` 创建 Coast 实例时被注入。这意味着你可以共享构建产物而不会暴露 secrets。

Secrets 可以在运行时重新注入而无需重新构建。在 [Coastguard](COASTGUARD.md) UI 中，在 Secrets 选项卡上使用 **Re-run Secrets** 操作。从 CLI 端，使用 [`coast build --refresh`](BUILDS.md) 重新提取并更新 secrets。

## TTL 与重新提取

Secrets 可以有一个可选的 `ttl`（time-to-live）字段。当 secret 过期时，`coast build --refresh` 会从来源重新提取它。

```toml
[secrets.short_lived_token]
extractor = "command"
run = "generate-token --ttl 1h"
inject = "env:AUTH_TOKEN"
ttl = "1h"
```

## 静态加密

所有已提取的 secrets 都会使用 AES-256-GCM 在本地密钥库中加密。加密密钥在 macOS 上存储于 macOS 钥匙串中，在 Linux 上则存储于一个权限为 0600 的文件中。

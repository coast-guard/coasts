# Secrets 与注入

`[secrets.*]` 小节定义了 Coast 在构建时从你的宿主机提取的凭据——钥匙串、环境变量、文件或任意命令——并将其作为环境变量或文件注入到 Coast 实例中。独立的 `[inject]` 小节会将宿主机上的非机密值转发到实例中，而无需提取或加密。

关于密钥在运行时如何存储、加密与管理，请参见 [Secrets](../concepts_and_terminology/SECRETS.md)。

## `[secrets.*]`

每个 secret 都是在 `[secrets]` 下的一个具名 TOML 小节。始终需要两个字段:`extractor` 和 `inject`。额外字段会作为参数传递给 extractor。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor`（必需）

提取方法的名称。内置 extractors:

- **`env`** — 读取宿主机环境变量
- **`file`** — 从宿主机文件系统读取文件
- **`command`** — 运行 shell 命令并捕获 stdout
- **`keychain`** — 从 macOS 钥匙串读取（仅 macOS）

你也可以使用自定义 extractor——任何在你的 PATH 上名为 `coast-extractor-{name}` 的可执行文件，都可以作为名为 `{name}` 的 extractor 使用。

### `inject`（必需）

secret 值在 Coast 实例内放置的位置。有两种格式:

- `"env:VAR_NAME"` — 作为环境变量注入
- `"file:/absolute/path"` — 写入文件（通过 tmpfs 挂载）

```toml
# 作为环境变量
inject = "env:DATABASE_URL"

# 作为文件
inject = "file:/run/secrets/db_password"
```

`env:` 或 `file:` 之后的值不能为空。

### `ttl`

可选的过期时长。超过该时间后，secret 会被视为过期（stale），Coast 会在下一次构建时重新运行 extractor。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### 额外参数

secret 小节中的任何额外键（除 `extractor`、`inject` 和 `ttl` 之外）都会作为参数传递给 extractor。需要哪些参数取决于 extractor。

## 内置 extractors

### `env` — 宿主机环境变量

按名称读取宿主机环境变量。

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

参数:`var` — 要读取的环境变量名。

### `file` — 宿主机文件

从宿主机文件系统读取文件内容。

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

参数:`path` — 宿主机上的文件路径。

### `command` — shell 命令

在宿主机上运行 shell 命令，并将 stdout 捕获为 secret 值。

```toml
[secrets.cmd_secret]
extractor = "command"
run = "echo command-secret-value"
inject = "env:CMD_SECRET"
```

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; d=json.load(open(\"$HOME/.claude.json\")); print(json.dumps({k:d[k] for k in [\"oauthAccount\"] if k in d}))"'
inject = "file:/root/.claude.json"
```

参数:`run` — 要执行的 shell 命令。

### `keychain` — macOS 钥匙串

从 macOS 钥匙串读取凭据。仅在 macOS 上可用——在其他平台引用该 extractor 会在构建时产生错误。

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

参数:`service` — 要查找的钥匙串服务名称。

## `[inject]`

`[inject]` 小节会将宿主机环境变量和文件转发到 Coast 实例中，而不经过 secret 提取与加密系统。将其用于你的服务需要的非敏感宿主机值。

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — 要转发的宿主机环境变量名列表
- **`files`** — 要挂载到实例中的宿主机文件路径列表

## 示例

### 多个 extractor

```toml
[secrets.file_secret]
extractor = "file"
path = "./test-secret.txt"
inject = "env:FILE_SECRET"

[secrets.env_secret]
extractor = "env"
var = "COAST_TEST_ENV_SECRET"
inject = "env:ENV_SECRET"

[secrets.cmd_secret]
extractor = "command"
run = "echo command-secret-value"
inject = "env:CMD_SECRET"

[secrets.file_inject_secret]
extractor = "file"
path = "./test-secret.txt"
inject = "file:/run/secrets/test_secret"
```

### 从 macOS 钥匙串进行 Claude Code 认证

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; d=json.load(open(\"$HOME/.claude.json\")); out={\"hasCompletedOnboarding\":True,\"numStartups\":1}; print(json.dumps(out))"'
inject = "file:/root/.claude.json"
```

### 带 TTL 的 secrets

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

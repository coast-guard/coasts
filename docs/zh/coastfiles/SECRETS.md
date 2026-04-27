# Secrets and Injection

`[secrets.*]` 部分定义了 Coast 在构建时从你的主机提取的凭据——钥匙串、环境变量、文件或任意命令——并将其作为环境变量或文件注入到 Coast 实例中。独立的 `[inject]` 部分则会将非秘密的主机值转发到实例中，而无需提取或加密。

关于密钥在运行时如何存储、加密和管理，请参见 [Secrets](../concepts_and_terminology/SECRETS.md)。

Secrets 与 [variable interpolation](VARIABLES.md) 不同。变量（`${VAR}`）在解析时被解析，其值会出现在构建产物中。Secrets 则在构建时被提取，并以加密形式存储在密钥库中——它们的值绝不会出现在构建产物中。

## `[secrets.*]`

每个 secret 都是在 `[secrets]` 下的一个具名 TOML 部分。始终需要两个字段:`extractor` 和 `inject`。其他字段会作为参数传递给提取器。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (required)

提取方法的名称。内置提取器:

- **`env`** — 读取主机环境变量
- **`file`** — 读取主机文件系统中的文件
- **`command`** — 运行 shell 命令并捕获 stdout
- **`keychain`** — 从 macOS Keychain 读取（仅限 macOS）

你也可以使用自定义提取器——任何在你的 PATH 上名为 `coast-extractor-{name}` 的可执行文件，都可以作为该名称的提取器使用。

### `inject` (required)

secret 值在 Coast 实例内部的放置位置。支持两种格式:

- `"env:VAR_NAME"` — 作为环境变量注入
- `"file:/absolute/path"` — 写入文件（通过 tmpfs 挂载）

```toml
# 作为环境变量
inject = "env:DATABASE_URL"

# 作为文件
inject = "file:/run/secrets/db_password"
```

`env:` 或 `file:` 后面的值不能为空。

### `ttl`

可选的过期时长。超过此时间后，secret 会被视为已过期，Coast 会在下一次构建时重新运行提取器。

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### Extra parameters

secret 部分中的任何附加键（除 `extractor`、`inject` 和 `ttl` 之外）都会作为参数传递给提取器。需要哪些参数取决于提取器。

## Built-in extractors

### `env` — host environment variable

按名称读取主机环境变量。

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

参数:`var` — 要读取的环境变量名称。

### `file` — host file

读取主机文件系统中文件的内容。

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

参数:`path` — 主机上的文件路径。

### `command` — shell command

在主机上运行 shell 命令，并将 stdout 捕获为 secret 值。

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

### `keychain` — macOS Keychain

从 macOS Keychain 读取凭据。仅在 macOS 上可用——在其他平台上引用此提取器会产生构建时错误。

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

参数:`service` — 要查找的 Keychain 服务名称。

## `[inject]`

`[inject]` 部分会将主机环境变量和文件直接转发到 Coast 实例中，而不会经过 secret 提取和加密系统。将其用于你的服务需要从主机获取的非敏感值。

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — 要转发的主机环境变量名称列表
- **`files`** — 要挂载到实例中的主机文件路径列表

## Examples

### Multiple extractors

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

### Claude Code authentication from macOS Keychain

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

### Secrets with TTL

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

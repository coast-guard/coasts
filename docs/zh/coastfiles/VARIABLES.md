# 变量

Coastfile 支持在所有字符串值中进行环境变量插值。变量在解析时、即 TOML 被处理之前被解析，因此它们可用于任何节和任何值位置。

## 语法

使用 `${VAR_NAME}` 引用环境变量:

```toml
[coast]
name = "${PROJECT_NAME}"
compose = "${COMPOSE_PATH}"

[ports]
web = ${WEB_PORT}
```

变量名必须以字母或下划线开头，后跟字母、数字或下划线（匹配模式 `[A-Za-z_][A-Za-z0-9_]*`）。

## 默认值

使用 `${VAR:-default}` 在变量未设置时提供回退值:

```toml
[coast]
name = "${PROJECT_NAME:-my-app}"
runtime = "${RUNTIME:-dind}"

[ports]
web = ${WEB_PORT:-3000}
api = ${API_PORT:-8080}
```

如果 `PROJECT_NAME` 在环境中已设置，则使用其值。否则，将替换为 `my-app`。默认值可以包含除 `}` 之外的任何字符。

## 未定义变量

当变量在没有默认值的情况下被引用，且在环境中未设置时，Coast **会保留字面量 `${VAR}` 文本** 并发出警告:

```
warning: undefined environment variable 'DB_HOST' preserved as literal '${DB_HOST}'; use '${DB_HOST:-}' for explicit empty, or '$${DB_HOST}' to escape entirely
```

保留该引用（而不是静默地将其替换为空字符串）可以让诸如 `ARCH=$(uname -m) && curl .../linux-${ARCH}.tar.gz` 这样的 shell 命令继续正常工作——即使 Coast 从未设置它，Dockerfile 的 shell 仍然可以在构建时展开 `${ARCH}`。

如果你确实希望在变量缺失时替换为空值，请使用显式空默认值:

```toml
[coast]
name = "${PROJECT_NAME:-}"   # 当 PROJECT_NAME 未设置时为 ""
```

如果你想要字面量 `${VAR}` 文本且不产生任何警告，请使用 `$${VAR}` 对其进行转义（见下文的 [转义](#escaping)）。

## 转义

要在 Coastfile 中生成字面量 `${...}`（例如，在某个值中应包含文本 `${VAR}` 而不是其展开值时），请将开头的美元符号写成两个:

```toml
[coast.setup]
run = ["echo '$${NOT_EXPANDED}'"]
```

这会生成字面字符串 `echo '${NOT_EXPANDED}'`，且不会尝试查找变量。

## 示例

### 使用环境来源键的密钥

```toml
[secrets.api_key]
extractor = "env"
var = "${API_KEY_ENV_VAR:-MY_API_KEY}"
inject = "env:API_KEY"
```

### 共享服务配置

```toml
[shared_services.postgres]
image = "postgres:${PG_VERSION:-16}"
env = [
    "POSTGRES_USER=${DB_USER:-coast}",
    "POSTGRES_PASSWORD=${DB_PASSWORD:-dev}",
    "POSTGRES_DB=${DB_NAME:-coast_dev}",
]
ports = [5432]
```

### 按环境区分的 compose 路径

```toml
[coast]
name = "my-app"
compose = "${COMPOSE_FILE:-./docker-compose.yml}"
```

## 变量与密钥

变量插值和 [密钥](SECRETS.md) 用途不同:

| | 变量 (`${VAR}`) | 密钥 (`[secrets.*]`) |
|---|---|---|
| **何时解析** | 解析时（TOML 处理之前） | 构建时（从配置的数据源中提取） |
| **存储位置** | 烘焙进已解析的 Coastfile 中 | 加密密钥库（`~/.coast/keystore.db`） |
| **使用场景** | 因环境而异的配置（端口、路径、镜像标签） | 敏感凭据（API 密钥、令牌、密码） |
| **在产物中可见** | 是（值会出现在构建内的 `coastfile.toml` 中） | 否（只有密钥名称会出现在清单中） |

对于在不同机器或 CI 环境之间变化的非敏感配置，请使用变量。对于绝不应出现在构建产物中的值，请使用密钥。

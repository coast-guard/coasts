# MCP-серверы и клиенты

> **Примечание:** Конфигурация MCP актуальна только тогда, когда вы запускаете coding agent внутри контейнера Coast через [`[agent_shell]`](AGENT_SHELL.md). Если ваш агент запускается на хосте (более распространённый вариант), у него уже есть доступ к собственным MCP-серверам, и Coast не нужно их настраивать.

Секции `[mcp.*]` настраивают MCP (Model Context Protocol) серверы, которые запускаются внутри или рядом с вашими экземплярами Coast. Секции `[mcp_clients.*]` подключают эти серверы к coding agents, таким как Claude Code или Cursor, чтобы они могли автоматически обнаруживать и использовать их.

О том, как MCP-серверы устанавливаются, проксируются и управляются во время выполнения, см. [MCP Servers](../concepts_and_terminology/MCP_SERVERS.md).

## MCP-серверы — `[mcp.*]`

Каждый MCP-сервер — это именованная секция TOML под `[mcp]`. Есть два режима: **внутренний** (запускается внутри контейнера Coast) и **проксируемый с хоста** (запускается на хосте и проксируется в Coast).

### Внутренние MCP-серверы

Внутренний сервер устанавливается и запускается внутри DinD-контейнера. Поле `command` обязательно, когда нет `proxy`.

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

Поля:

- **`command`** (обязательно) — исполняемый файл для запуска
- **`args`** — аргументы, передаваемые команде
- **`env`** — переменные окружения для процесса сервера
- **`install`** — команды, выполняемые перед запуском сервера (принимает строку или массив)
- **`source`** — директория на хосте, которую нужно скопировать в контейнер по пути `/mcp/{name}/`

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]
```

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

### MCP-серверы, проксируемые с хоста

Сервер, проксируемый с хоста, запускается на вашей хост-машине и становится доступен внутри Coast через `coast-mcp-proxy`. Установите `proxy = "host"`, чтобы включить этот режим.

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

Когда `proxy = "host"`:

- `command`, `args` и `env` необязательны — если они опущены, сервер будет определён по имени из существующей конфигурации MCP на хосте.
- `install` и `source` **запрещены** (сервер запускается на хосте, а не в контейнере).

Сервер, проксируемый с хоста, без дополнительных полей ищет сервер по имени в конфигурации хоста:

```toml
[mcp.host-lookup]
proxy = "host"
```

Единственное допустимое значение для `proxy` — `"host"`.

### Несколько серверов

Вы можете определить любое количество MCP-серверов:

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]

[mcp.host-lookup]
proxy = "host"
```

## MCP-клиенты — `[mcp_clients.*]`

Коннекторы MCP-клиентов сообщают Coast, как записывать конфигурацию MCP-серверов в файлы конфигурации, которые читают coding agents. Это автоматически подключает ваши серверы `[mcp.*]` к агентам.

### Встроенные коннекторы

Встроены два коннектора: `claude-code` и `cursor`. Их использование не требует дополнительных полей.

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

Встроенные коннекторы автоматически знают:

- **`claude-code`** — записывает в `/root/.claude/mcp_servers.json`
- **`cursor`** — записывает в `/workspace/.cursor/mcp.json`

Вы можете переопределить путь к конфигурации:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### Пользовательские коннекторы

Для агентов, которых нет среди встроенных, используйте поле `run`, чтобы указать команду оболочки, которую Coast выполняет для регистрации MCP-серверов:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

Поле `run` нельзя комбинировать с `format` или `config_path`.

### Коннекторы с пользовательским форматом

Если ваш агент использует тот же формат файла конфигурации, что и Claude Code или Cursor, но находится по другому пути:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

`format` должен быть `"claude-code"` или `"cursor"`. При использовании не встроенного имени вместе с `format` поле `config_path` обязательно.

## Примеры

### Внутренний MCP-сервер, подключённый к Claude Code

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### Сервер, проксируемый с хоста, вместе с внутренним сервером

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }

[mcp_clients.claude-code]
```

### Несколько коннекторов клиентов

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```

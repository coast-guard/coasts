# Servidores y Clientes MCP

> **Nota:** La configuración de MCP solo es relevante cuando estás ejecutando un agente de programación dentro de un contenedor de Coast mediante [`[agent_shell]`](AGENT_SHELL.md). Si tu agente se ejecuta en el host (la configuración más común), ya tiene acceso a sus propios servidores MCP y no necesita que Coast los configure.

Las secciones `[mcp.*]` configuran servidores MCP (Model Context Protocol) que se ejecutan dentro o junto a tus instancias de Coast. Las secciones `[mcp_clients.*]` conectan esos servidores con agentes de programación como Claude Code o Cursor para que puedan descubrirlos y usarlos automáticamente.

Para saber cómo se instalan, se proxifican y se gestionan en tiempo de ejecución los servidores MCP, consulta [Servidores MCP](../concepts_and_terminology/MCP_SERVERS.md).

## Servidores MCP — `[mcp.*]`

Cada servidor MCP es una sección TOML con nombre bajo `[mcp]`. Hay dos modos: **interno** (se ejecuta dentro del contenedor de Coast) y **proxificado desde el host** (se ejecuta en el host, proxificado hacia Coast).

### Servidores MCP internos

Un servidor interno se instala y se ejecuta dentro del contenedor DinD. El campo `command` es obligatorio cuando no hay `proxy`.

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

Campos:

- **`command`** (obligatorio) — el ejecutable a ejecutar
- **`args`** — argumentos que se pasan al comando
- **`env`** — variables de entorno para el proceso del servidor
- **`install`** — comandos a ejecutar antes de iniciar el servidor (acepta una cadena o un arreglo)
- **`source`** — un directorio del host para copiar dentro del contenedor en `/mcp/{name}/`

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

### Servidores MCP proxificados desde el host

Un servidor proxificado desde el host se ejecuta en tu máquina host y se pone a disposición dentro de Coast mediante `coast-mcp-proxy`. Establece `proxy = "host"` para habilitar este modo.

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

Cuando `proxy = "host"`:

- `command`, `args` y `env` son opcionales — si se omiten, el servidor se resuelve por nombre a partir de la configuración MCP existente del host.
- `install` y `source` **no están permitidos** (el servidor se ejecuta en el host, no en el contenedor).

Un servidor proxificado desde el host sin campos adicionales busca el servidor por nombre en la configuración del host:

```toml
[mcp.host-lookup]
proxy = "host"
```

El único valor válido para `proxy` es `"host"`.

### Múltiples servidores

Puedes definir cualquier número de servidores MCP:

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

## Clientes MCP — `[mcp_clients.*]`

Los conectores de clientes MCP le indican a Coast cómo escribir la configuración de servidores MCP en los archivos de configuración que leen los agentes de programación. Esto conecta automáticamente tus servidores `[mcp.*]` con los agentes.

### Conectores integrados

Hay dos conectores integrados: `claude-code` y `cursor`. Usarlos no requiere campos adicionales.

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

Los conectores integrados saben automáticamente:

- **`claude-code`** — escribe en `/root/.claude/mcp_servers.json`
- **`cursor`** — escribe en `/workspace/.cursor/mcp.json`

Puedes sobrescribir la ruta de configuración:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### Conectores personalizados

Para agentes que no están integrados, usa el campo `run` para especificar un comando de shell que Coast ejecuta para registrar servidores MCP:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

El campo `run` no se puede combinar con `format` o `config_path`.

### Conectores de formato personalizado

Si tu agente usa el mismo formato de archivo de configuración que Claude Code o Cursor pero vive en una ruta diferente:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

El `format` debe ser `"claude-code"` o `"cursor"`. Al usar un nombre no integrado con `format`, `config_path` es obligatorio.

## Ejemplos

### Servidor MCP interno conectado a Claude Code

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### Servidor proxificado desde el host con servidor interno

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

### Múltiples conectores de cliente

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```

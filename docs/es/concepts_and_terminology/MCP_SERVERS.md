# Servidores MCP

Los servidores MCP (Model Context Protocol) dan a los agentes de IA acceso a herramientas — búsqueda de archivos, consultas a bases de datos, búsqueda en documentación, automatización del navegador y más. Coast puede instalar y configurar servidores MCP dentro de un contenedor de Coast para que un agente en contenedor tenga acceso a las herramientas que necesita.

**Esto solo es relevante si estás ejecutando tu agente dentro del contenedor de Coast.** Si ejecutas agentes en el host (el enfoque recomendado), tus servidores MCP también se ejecutan en el host y no se necesita ninguna de esta configuración. Esta página se basa en [Agent Shells](AGENT_SHELLS.md) y añade otra capa de complejidad encima. Lee las advertencias allí antes de continuar.

## Servidores internos vs proxificados al host

Coast admite dos modos para servidores MCP, controlados por el campo `proxy` en la sección `[mcp]` de tu Coastfile.

### Servidores internos

Los servidores internos se instalan y se ejecutan dentro del contenedor DinD en `/mcp/<name>/`. Tienen acceso directo al sistema de archivos en contenedor y a los servicios en ejecución.

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

También puedes copiar archivos fuente de tu proyecto al directorio MCP:

```toml
[mcp.my-custom-tool]
source = "tools/my-mcp-server"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
```

El campo `source` copia archivos desde `/workspace/<path>/` a `/mcp/<name>/` durante la configuración. Los comandos `install` se ejecutan dentro de ese directorio. Esto es útil para servidores MCP que viven en tu repositorio.

### Servidores proxificados al host

Los servidores proxificados al host se ejecutan en tu máquina host, no dentro del contenedor. Coast genera una configuración de cliente que usa `coast-mcp-proxy` para reenviar solicitudes MCP desde el contenedor al host a través de la red.

```toml
[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]
```

Los servidores proxificados al host no pueden tener campos `install` o `source` — se espera que ya estén disponibles en el host. Usa este modo para servidores MCP que necesitan acceso a nivel de host, como automatización del navegador o herramientas del sistema de archivos del host.

### Cuándo usar cada uno

| Modo | Se ejecuta en | Adecuado para | Limitaciones |
|---|---|---|---|
| Interno | Contenedor DinD | Herramientas que necesitan acceso al sistema de archivos del contenedor, herramientas específicas del proyecto | Debe poder instalarse en Alpine Linux, añade tiempo a `coast run` |
| Proxificado al host | Máquina host | Automatización del navegador, herramientas a nivel de host, servidores grandes preinstalados | No puede acceder directamente al sistema de archivos del contenedor |

## Conectores de cliente

La sección `[mcp_clients]` le indica a Coast dónde escribir la configuración generada del servidor MCP para que el agente dentro del contenedor pueda descubrir los servidores.

### Formatos integrados

Para Claude Code y Cursor, basta con una sección vacía con el nombre correcto — Coast detecta automáticamente el formato y la ruta de configuración predeterminada:

```toml
[mcp_clients.claude-code]
# Writes to /root/.claude/mcp_servers.json (auto-detected)

[mcp_clients.cursor]
# Writes to /workspace/.cursor/mcp.json (auto-detected)
```

### Ruta de configuración personalizada

Para otras herramientas de IA, especifica el formato y la ruta explícitamente:

```toml
[mcp_clients.my-tool]
format = "claude-code"
config_path = "/home/coast/.config/my-tool/mcp.json"
```

### Conectores basados en comandos

En lugar de escribir un archivo, puedes canalizar el JSON de configuración generado hacia un comando:

```toml
[mcp_clients.custom-setup]
run = "my-config-tool import-mcp --stdin"
```

El campo `run` es mutuamente excluyente con `format` y `config_path`.

## Pestaña MCP de Coastguard

La interfaz web de [Coastguard](COASTGUARD.md) proporciona visibilidad de tu configuración MCP desde la pestaña MCP.

![MCP tab in Coastguard](../../assets/coastguard-mcp.png)
*La pestaña MCP de Coastguard mostrando servidores configurados, sus herramientas y ubicaciones de configuración del cliente.*

La pestaña tiene tres secciones:

- **MCP Servers** — enumera cada servidor declarado con su nombre, tipo (Interno o Host), comando y estado (Instalado, Proxificado o No instalado).
- **Tools** — selecciona un servidor para inspeccionar las herramientas que expone mediante el protocolo MCP. Cada herramienta muestra su nombre y descripción; haz clic para ver el esquema completo de entrada.
- **Client Locations** — muestra dónde se escribieron los archivos de configuración generados (p. ej., formato `claude-code` en `/root/.claude/mcp_servers.json`).

## Comandos de CLI

```bash
coast mcp dev-1 ls                          # list servers with type and status
coast mcp dev-1 tools context7              # list tools exposed by a server
coast mcp dev-1 tools context7 info resolve # show input schema for a specific tool
coast mcp dev-1 locations                   # show where client configs were written
```

El comando `tools` funciona enviando solicitudes JSON-RPC `initialize` y `tools/list` al proceso del servidor MCP dentro del contenedor. Solo funciona para servidores internos — los servidores proxificados al host deben inspeccionarse desde el host.

## Cómo funciona la instalación

Durante `coast run`, después de que el daemon Docker interno esté listo y los servicios estén iniciándose, Coast configura MCP:

1. Para cada servidor MCP **interno**:
   - Crea `/mcp/<name>/` dentro del contenedor DinD
   - Si `source` está establecido, copia archivos desde `/workspace/<source>/` a `/mcp/<name>/`
   - Ejecuta cada comando `install` dentro de `/mcp/<name>/` (p. ej., `npm install -g @upstash/context7-mcp`)

2. Para cada **conector de cliente**:
   - Genera la configuración JSON en el formato apropiado (Claude Code o Cursor)
   - Los servidores internos obtienen su `command` y `args` reales con `cwd` establecido en `/mcp/<name>/`
   - Los servidores proxificados al host obtienen `coast-mcp-proxy` como comando con el nombre del servidor como argumento
   - Escribe la configuración en la ruta de destino (o la canaliza al comando `run`)

Los servidores proxificados al host dependen de `coast-mcp-proxy` dentro del contenedor para reenviar solicitudes del protocolo MCP de vuelta a la máquina host, donde se ejecuta el proceso real del servidor MCP.

## Ejemplo completo

Un Coastfile que configura una herramienta interna de documentación y una herramienta de navegador proxificada al host, conectadas a Claude Code:

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]

[mcp_clients.claude-code]
```

Después de `coast run`, Claude Code dentro del contenedor ve ambos servidores en su configuración MCP — `context7` ejecutándose localmente en `/mcp/context7/` y `browser` proxificado al host.

## Agentes ejecutándose en el host

Si tu agente de programación se ejecuta en la máquina host (el enfoque recomendado), tus servidores MCP también se ejecutan en el host y la configuración `[mcp]` de Coast no interviene. Sin embargo, hay una cosa a considerar: **los servidores MCP que se conectan a bases de datos o servicios dentro de un Coast necesitan conocer el puerto correcto.**

Cuando los servicios se ejecutan dentro de un Coast, son accesibles en puertos dinámicos que cambian cada vez que ejecutas una nueva instancia. Un MCP de base de datos en el host que se conecta a `localhost:5432` solo alcanzará la base de datos del Coast [checked-out](CHECKOUT.md) — o nada en absoluto si no hay ningún Coast checked-out. Para instancias no checked-out, necesitarías reconfigurar el MCP para usar el [puerto dinámico](PORTS.md) (p. ej., `localhost:55681`).

Hay dos maneras de evitar esto:

**Usa servicios compartidos.** Si tu base de datos se ejecuta como un [servicio compartido](SHARED_SERVICES.md), vive en el daemon Docker del host en su puerto canónico (`localhost:5432`). Cada instancia de Coast se conecta a ella a través de una red puente, y tu MCP del lado del host se conecta a la misma base de datos en el mismo puerto que siempre ha tenido. No se necesita reconfiguración, ni descubrimiento de puertos dinámicos. Este es el enfoque más sencillo.

**Usa `coast exec` o `coast docker`.** Si tu base de datos se ejecuta dentro del Coast (volúmenes aislados), tu agente del lado del host aún puede consultarla ejecutando comandos a través de Coast (ver [Exec & Docker](EXEC_AND_DOCKER.md)):

```bash
coast exec dev-1 -- psql -h localhost -U myuser -d mydb -c "SELECT count(*) FROM users"
coast docker dev-1 exec -i my-postgres psql -U myuser -d mydb -c "\\dt"
```

Esto evita por completo la necesidad de conocer el puerto dinámico — el comando se ejecuta dentro del Coast donde la base de datos está en su puerto canónico.

Para la mayoría de los flujos de trabajo, los servicios compartidos son el camino de menor resistencia. La configuración de tu MCP en el host se mantiene exactamente igual que antes de empezar a usar Coasts.

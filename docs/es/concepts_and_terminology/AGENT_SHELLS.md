# Shells de agente

Los shells de agente son shells dentro de un Coast que se abren directamente a un runtime TUI de agente — Claude Code, Codex o cualquier agente CLI. Los configuras con una sección `[agent_shell]` en tu Coastfile y Coast lanza el proceso del agente dentro del contenedor DinD.

**Para la mayoría de los casos de uso, no deberías hacer esto.** En su lugar, ejecuta tus agentes de programación en la máquina host. El [filesystem](FILESYSTEM.md) compartido significa que un agente en el host puede editar código con normalidad mientras llama a [`coast logs`](LOGS.md), [`coast exec`](EXEC_AND_DOCKER.md) y [`coast ps`](RUNTIMES_AND_SERVICES.md) para obtener información del runtime. Los shells de agente añaden montaje de credenciales, complicaciones de OAuth y complejidad del ciclo de vida que no necesitas a menos que tengas una razón específica para contenerizar el propio agente.

## El problema de OAuth

Si estás usando Claude Code, Codex u herramientas similares que autentican mediante OAuth, el token fue emitido para tu máquina host. Cuando ese mismo token se usa desde dentro de un contenedor Linux — un user agent distinto, un entorno distinto — el proveedor puede marcarlo o revocarlo. Obtendrás fallos de autenticación intermitentes que son difíciles de depurar.

Para agentes en contenedores, la autenticación basada en API key es la opción más segura. Establece la clave como un [secret](SECRETS.md) en tu Coastfile e inyéctala en el entorno del contenedor.

Si las API keys no son una opción, puedes montar credenciales OAuth dentro del Coast (consulta la sección Configuración más abajo), pero espera fricción. En macOS, si usas el extractor de secretos `keychain` para obtener tokens OAuth, cada `coast build` te pedirá la contraseña del llavero de macOS. Esto hace que el proceso de build sea tedioso, especialmente cuando reconstruyes con frecuencia. El aviso del Keychain es un requisito de seguridad de macOS y no se puede omitir.

## Configuración

Añade una sección `[agent_shell]` a tu Coastfile con el comando a ejecutar:

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

El comando se ejecuta dentro del contenedor DinD en `/workspace`. Coast crea un usuario `coast` dentro del contenedor, copia credenciales de `/root/.claude/` a `/home/coast/.claude/` y ejecuta el comando como ese usuario. Si tu agente necesita credenciales montadas dentro del contenedor, usa `[secrets]` con inyección de archivos (ver [Secrets and Extractors](SECRETS.md)) y `[coast.setup]` para instalar la CLI del agente:

```toml
[coast.setup]
run = ["npm install -g @anthropic-ai/claude-code"]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "claude --dangerously-skip-permissions"
```

Si se configura `[agent_shell]`, Coast lanza automáticamente un shell cuando inicia la instancia. La configuración se hereda mediante `extends` y puede sobrescribirse por [tipo de Coastfile](COASTFILE_TYPES.md).

## El modelo de agente activo

Cada instancia de Coast puede tener múltiples shells de agente, pero solo uno está **activo** a la vez. El shell activo es el destino predeterminado para los comandos que no especifican un ID `--shell`.

```bash
coast agent-shell dev-1 ls

  SHELL  STATUS   ACTIVE
  1      running  ★
  2      running
```

Cambiar el shell activo:

```bash
coast agent-shell dev-1 activate 2
```

No puedes cerrar el shell activo — primero activa uno diferente. Esto evita matar accidentalmente el shell con el que estás interactuando.

En Coastguard, los shells de agente aparecen como pestañas en el panel Exec con insignias de activo/inactivo. Haz clic en una pestaña para ver su terminal; usa el menú desplegable para activar, lanzar o cerrar shells.

![Agent shell in Coastguard](../../assets/coastguard-agent-shell.png)
*Un shell de agente ejecutando Claude Code dentro de una instancia de Coast, accesible desde la pestaña Exec en Coastguard.*

## Envío de entrada

La forma principal de controlar programáticamente un agente contenerizado es `coast agent-shell input`:

```bash
coast agent-shell dev-1 input "fix the failing test in auth.test.ts"
```

Esto escribe el texto en el TUI del agente activo y pulsa Enter. El agente lo recibe como si lo hubieras tecleado en el terminal.

Opciones:

- `--no-send` — escribe el texto sin pulsar Enter. Útil para ir construyendo una entrada parcial o navegar menús TUI.
- `--shell <id>` — apunta a un shell específico en lugar del activo.
- `--show-bytes` — imprime los bytes exactos que se envían, para depuración.

Bajo el capó, la entrada se escribe directamente en el descriptor de archivo maestro del PTY. El texto y la pulsación de Enter se envían como dos escrituras separadas con una pausa de 25ms para evitar artefactos de modo pegado que algunos frameworks TUI muestran al recibir entrada rápida.

## Otros comandos

```bash
coast agent-shell dev-1 spawn              # create a new shell
coast agent-shell dev-1 spawn --activate   # create and immediately activate
coast agent-shell dev-1 tty                # attach interactive TTY to active shell
coast agent-shell dev-1 tty --shell 2      # attach to a specific shell
coast agent-shell dev-1 read-output        # read full scrollback buffer
coast agent-shell dev-1 read-last-lines 50 # read last 50 lines of output
coast agent-shell dev-1 session-status     # check if the shell process is alive
```

`tty` te ofrece una sesión interactiva en vivo — puedes escribir directamente en el TUI del agente. Desacopla con la secuencia de escape estándar del terminal. `read-output` y `read-last-lines` son no interactivos y devuelven texto, lo cual es útil para scripting y automatización.

## Ciclo de vida y recuperación

Las sesiones de shell de agente persisten en Coastguard al navegar entre páginas. El búfer de scrollback (hasta 512KB) se reproduce cuando te vuelves a conectar a una pestaña.

Cuando detienes una instancia de Coast con `coast stop`, se matan todos los procesos PTY de los shells de agente y se limpian sus registros en la base de datos. `coast start` lanza automáticamente un shell de agente nuevo si `[agent_shell]` está configurado.

Tras un reinicio del daemon, los shells de agente que estaban ejecutándose previamente se mostrarán como muertos. El sistema lo detecta automáticamente — si el shell activo está muerto, el primer shell vivo se promociona a activo. Si no hay shells vivos, lanza uno nuevo con `coast agent-shell spawn --activate`.

## Para quién es esto

Los shells de agente están diseñados para **productos que construyen integraciones de primera parte** alrededor de Coasts — plataformas de orquestación, wrappers de agentes y herramientas que quieren gestionar agentes de programación contenerizados de forma programática mediante las APIs `input`, `read-output` y `session-status`.

Para programación general con agentes en paralelo, ejecuta los agentes en el host. Es más simple, evita problemas de OAuth, elude la complejidad del montaje de credenciales y aprovecha al máximo el filesystem compartido. Obtienes todos los beneficios de Coast (runtimes aislados, gestión de puertos, cambio de worktree) sin ninguna de las sobrecargas de contenerización del agente.

El siguiente nivel de complejidad más allá de los shells de agente es montar [servidores MCP](MCP_SERVERS.md) dentro del Coast para que el agente contenerizado tenga acceso a herramientas. Esto amplía aún más la superficie de integración y se cubre por separado. La capacidad está ahí si la necesitas, pero la mayoría de los usuarios no deberían.

# Shell del agente

> **En la mayoría de los flujos de trabajo, no necesitas contenerizar tu agente de codificación.** Dado que Coast comparte el [sistema de archivos](../concepts_and_terminology/FILESYSTEM.md) con tu máquina host, el enfoque más simple es ejecutar el agente en tu host y usar [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md) para tareas pesadas en tiempo de ejecución como pruebas de integración. Los shells de agente son para casos en los que específicamente quieres que el agente se ejecute dentro del contenedor — por ejemplo, para darle acceso directo al daemon interno de Docker o para aislar completamente su entorno.

La sección `[agent_shell]` configura una TUI de agente — como Claude Code o Codex — para ejecutarse dentro del contenedor de Coast. Cuando está presente, Coast inicia automáticamente una sesión PTY persistente ejecutando el comando configurado cuando se inicia una instancia.

Para ver el panorama completo de cómo funcionan los shells de agente — el modelo de agente activo, el envío de entrada, el ciclo de vida y la recuperación — consulta [Agent Shells](../concepts_and_terminology/AGENT_SHELLS.md).

## Configuración

La sección tiene un único campo obligatorio: `command`.

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (obligatorio)

El comando de shell que se ejecutará en el PTY del agente. Esto suele ser un CLI de agente de codificación que has instalado mediante `[coast.setup]`.

El comando se ejecuta dentro del contenedor DinD en `/workspace` (la raíz del proyecto). No es un servicio de compose — se ejecuta junto a tu stack de compose o servicios sin compose, no dentro de ellos.

## Ciclo de vida

- El shell del agente se inicia automáticamente con `coast run`.
- En [Coastguard](../concepts_and_terminology/COASTGUARD.md), aparece como una pestaña persistente "Agent" que no se puede cerrar.
- Si el proceso del agente termina, Coast puede reiniciarlo.
- Puedes enviar entrada a un shell de agente en ejecución mediante `coast agent-shell input`.

## Ejemplos

### Claude Code

Instala Claude Code en `[coast.setup]`, configura las credenciales mediante [secrets](SECRETS.md), luego configura el shell del agente:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git", "bash"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "mkdir -p /root/.claude",
]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "cd /workspace; exec claude --dangerously-skip-permissions --effort high"
```

### Shell de agente simple

Un shell de agente mínimo para probar que la funcionalidad funciona:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```

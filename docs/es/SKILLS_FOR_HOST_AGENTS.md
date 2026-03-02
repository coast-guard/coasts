# Habilidades para Agentes en el Host

Si estás usando agentes de codificación con IA (Claude Code, Codex, Conductor, Cursor o similares) en un proyecto que usa Coasts, tu agente necesita una habilidad que le enseñe cómo interactuar con el runtime de Coast. Sin esto, el agente editará archivos, pero no sabrá cómo ejecutar tests, revisar logs o verificar que sus cambios funcionen dentro del entorno en ejecución.

Esta guía explica cómo configurar esa habilidad.

## Por qué los Agentes Necesitan Esto

Coasts comparten el [filesystem](concepts_and_terminology/FILESYSTEM.md) entre tu máquina host y el contenedor de Coast. Tu agente edita archivos en el host y los servicios en ejecución dentro del Coast ven los cambios de inmediato. Pero el agente aún necesita:

1. **Descubrir con qué instancia de Coast está trabajando** — `coast lookup` lo resuelve a partir del directorio actual del agente.
2. **Ejecutar comandos dentro del Coast** — los tests, builds y otras tareas de runtime ocurren dentro del contenedor mediante `coast exec`.
3. **Leer logs y comprobar el estado de los servicios** — `coast logs` y `coast ps` le dan al agente feedback del runtime.

La habilidad de abajo enseña al agente las tres.

## La Habilidad

Agrega lo siguiente a la habilidad, reglas o archivo de prompt existente de tu agente. Si tu agente ya tiene instrucciones para ejecutar tests o interactuar con tu entorno de desarrollo, esto debe ir junto a esas — le enseña al agente cómo usar Coasts para operaciones de runtime.

```text-copy
This project uses Coasts (containerized host) for isolated development environments.
Your code edits are automatically visible inside the running Coast — the filesystem
is shared between the host and the container.

=== ORIENTATION ===

Before running any runtime commands, discover which Coast instance matches your
current working directory:

  coast lookup

This prints the instance name, ports, URLs, and example commands. Use the instance
name from the output for all subsequent commands.

If you need deeper context on how Coasts work, read these docs:

  coast docs --path concepts_and_terminology/LOOKUP.md
  coast docs --path concepts_and_terminology/FILESYSTEM.md
  coast docs --path concepts_and_terminology/EXEC_AND_DOCKER.md
  coast docs --path concepts_and_terminology/LOGS.md

=== RUNNING COMMANDS ===

Use `coast exec` to run commands inside the Coast. The shell starts at the workspace
root (where the Coastfile is). cd to your target directory first:

  coast exec <instance> -- sh -c "cd <dir> && <command>"

Examples:

  coast exec dev-1 -- sh -c "cd src && npm test"
  coast exec dev-1 -- sh -c "cd backend && go test ./..."
  coast exec dev-1 -- sh -c "cd apps/web && npx playwright test"

=== RUNTIME FEEDBACK ===

Check service status:

  coast ps <instance>

Read service logs:

  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

=== TROUBLESHOOTING ===

If you encounter errors or unfamiliar behavior, search the Coast docs:

  coast search-docs "error message or description"

This uses semantic search — describe the problem in natural language and it will
find the relevant documentation.

=== RULES ===

- Always run `coast lookup` before your first runtime command in a session.
- Do not run services directly on the host. Use `coast exec` for all runtime tasks.
- File edits on the host are instantly visible inside the Coast. You do not need
  to copy files or rebuild after editing.
- If `coast lookup` returns no instances, the Coast may not be running. Suggest
  `coast run dev-1` or check `coast ls` for the project state.
```

## Añadir la Habilidad a Tu Agente

La forma más rápida es dejar que el agente se configure a sí mismo. Ejecuta uno de estos desde tu directorio del proyecto:

```sh
# Claude Code
claude -p "$(coast skills-prompt)"

# Codex
codex "$(coast skills-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast skills-prompt)"
```

Esto le da al agente el texto de la habilidad e instrucciones para escribirlo en su propio archivo de configuración (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/coast.md`, etc.).

### Configuración manual

Si prefieres añadir la habilidad tú mismo:

- **Claude Code:** Añade el texto de la habilidad al archivo `CLAUDE.md` de tu proyecto.
- **Codex:** Añade el texto de la habilidad al archivo `AGENTS.md` de tu proyecto.
- **Cursor:** Crea `.cursor/rules/coast.md` en la raíz de tu proyecto y pega el texto de la habilidad.
- **Otros agentes:** Pega el texto de la habilidad en el prompt a nivel de proyecto o archivo de reglas que tu agente lea al iniciar.

## Lecturas adicionales

- Lee la [documentación de Coastfiles](coastfiles/README.md) para aprender el esquema completo de configuración
- Aprende los comandos del [Coast CLI](concepts_and_terminology/CLI.md) para gestionar instancias
- Explora [Coastguard](concepts_and_terminology/COASTGUARD.md), la UI web para observar y controlar tus Coasts
- Revisa [Conceptos y Terminología](concepts_and_terminology/README.md) para tener una visión completa de cómo funciona Coasts

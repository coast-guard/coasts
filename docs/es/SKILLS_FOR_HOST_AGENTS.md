# Habilidades para agentes host

Si usas agentes de codificación con IA en el host mientras tu aplicación se
ejecuta dentro de Coasts, tu agente normalmente necesita dos piezas de
configuración específicas de Coast:

1. una sección de Coast Runtime siempre activa en el archivo de instrucciones
   del proyecto o archivo de reglas del harness
2. una habilidad reutilizable de flujo de trabajo de Coast como `/coasts`
   cuando el harness admite habilidades de proyecto

Sin la primera pieza, el agente edita archivos pero olvida usar `coast exec`.
Sin la segunda, cada asignación de Coast, registro y flujo de UI tiene que
volver a explicarse en el chat.

Esta guía mantiene la configuración concreta y específica de Coast: qué archivo
crear, qué texto va en él y cómo cambia según el harness.

## Por qué los agentes necesitan esto

Coasts comparte el [sistema de archivos](concepts_and_terminology/FILESYSTEM.md) entre
tu máquina host y el contenedor de Coast. Tu agente edita archivos en el host
y los servicios en ejecución dentro del Coast ven los cambios inmediatamente.
Pero el agente aún necesita:

1. descubrir qué instancia de Coast coincide con el checkout actual
2. ejecutar pruebas, compilaciones y comandos de runtime dentro de ese Coast
3. leer registros y estado de servicios desde el Coast
4. manejar de forma segura la asignación de worktree cuando todavía no hay un Coast adjunto

## Qué va en cada lugar

- `AGENTS.md`, `CLAUDE.md` o `.cursor/rules/coast.md` — reglas cortas de Coast
  que deben aplicarse en cada tarea, incluso si no se invoca ninguna habilidad
- habilidad (`.agents/skills/...`, `.claude/skills/...` o `.cursor/skills/...`)
  — el propio flujo de trabajo reutilizable de Coast, como `/coasts`
- archivo de comando (`.claude/commands/...` o `.cursor/commands/...`) — punto
  de entrada explícito opcional para harnesses que lo admiten; una opción
  sencilla es hacer que el comando reutilice la habilidad

Si un repositorio usa más de un harness, mantén la habilidad canónica de Coast
en un solo lugar y expónla donde sea necesario. Consulta
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md).

## 1. Reglas siempre activas de Coast Runtime

Agrega el siguiente bloque al archivo de instrucciones del proyecto siempre
activo del harness o al archivo de reglas (`AGENTS.md`, `CLAUDE.md`,
`.cursor/rules/coast.md` o equivalente):

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

Este bloque pertenece al archivo siempre activo porque las reglas deben
aplicarse en cada tarea, no solo cuando el agente entra explícitamente en un
flujo de trabajo `/coasts`.

## 2. Habilidad reutilizable `/coasts`

Cuando el harness admite habilidades de proyecto, guarda el contenido de la
habilidad como un `SKILL.md` en tu directorio de habilidades. El texto completo
de la habilidad está en [skills_prompt.txt](skills_prompt.txt) (si estás en modo CLI, usa
`coast skills-prompt`) — todo lo que aparece después del bloque Coast Runtime
es el contenido de la habilidad, comenzando desde el frontmatter `---`.

Si estás usando superficies específicas de Codex u OpenAI, opcionalmente puedes
agregar `agents/openai.yaml` junto a la habilidad para metadatos de visualización
o política de invocación. Esos metadatos deben vivir junto a la habilidad, no
reemplazarla.

## Inicio rápido por harness

| Harness | Archivo siempre activo | Flujo de trabajo reutilizable de Coast | Notas |
|---------|------------------------|----------------------------------------|-------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | No hay un archivo de comando de proyecto separado que recomendar para la documentación de Coast. Consulta [Codex](harnesses/CODEX.md). |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md` es opcional, pero mantén la lógica en la habilidad. Consulta [Claude Code](harnesses/CLAUDE_CODE.md). |
| Cursor | `AGENTS.md` o `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` o `.agents/skills/coasts/SKILL.md` compartido | `.cursor/commands/coasts.md` es opcional. `.cursor/worktrees.json` es para el bootstrap de worktree de Cursor, no para la política de Coast. Consulta [Cursor](harnesses/CURSOR.md). |
| Conductor | `CLAUDE.md` | Empieza con `CLAUDE.md`; usa scripts y ajustes de Conductor para comportamiento específico de Conductor | No asumas el comportamiento completo de comandos de proyecto de Claude Code. Si un comando nuevo no aparece, cierra y vuelve a abrir Conductor por completo. Consulta [Conductor](harnesses/CONDUCTOR.md). |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Esta es la superficie de harness más limitada aquí. Usa la disposición estilo Codex y no inventes una capa de comandos nativa de T3 para la documentación de Coast. Consulta [T3 Code](harnesses/T3_CODE.md). |

## Deja que el agente se configure a sí mismo

La forma más rápida es dejar que el agente escriba por sí mismo los archivos
correctos. Copia el prompt de abajo en el chat de tu agente — incluye el bloque
de Coast Runtime, el bloque de habilidad `coasts` y las instrucciones
específicas del harness sobre dónde pertenece cada pieza.

```prompt-copy
skills_prompt.txt
```

También puedes obtener la misma salida desde la CLI ejecutando `coast skills-prompt`.

## Configuración manual

- **Codex:** coloca la sección Coast Runtime en `AGENTS.md`, luego coloca la
  habilidad reutilizable `coasts` en `.agents/skills/coasts/SKILL.md`.
- **Claude Code:** coloca la sección Coast Runtime en `CLAUDE.md`, luego coloca la
  habilidad reutilizable `coasts` en `.claude/skills/coasts/SKILL.md`. Solo agrega
  `.claude/commands/coasts.md` si específicamente quieres un archivo de comando.
- **Cursor:** coloca la sección Coast Runtime en `AGENTS.md` si quieres las
  instrucciones más portables, o en `.cursor/rules/coast.md` si quieres una
  regla de proyecto nativa de Cursor. Coloca el flujo de trabajo reutilizable
  `coasts` en `.cursor/skills/coasts/SKILL.md` para un repositorio solo de
  Cursor, o en `.agents/skills/coasts/SKILL.md` si el repositorio se comparte
  con otros harnesses. Solo agrega `.cursor/commands/coasts.md` si
  específicamente quieres un archivo de comando explícito.
- **Conductor:** coloca la sección Coast Runtime en `CLAUDE.md`. Usa los scripts
  de Repository Settings de Conductor para bootstrap o comportamiento de
  ejecución específicos de Conductor. Si agregas un comando y no aparece,
  cierra y vuelve a abrir por completo la aplicación.
- **T3 Code:** usa la misma disposición que Codex: `AGENTS.md` más
  `.agents/skills/coasts/SKILL.md`. Trata T3 Code aquí como un harness
  ligero de estilo Codex, no como una superficie separada de comandos de Coast.
- **Multiple harnesses:** mantén la habilidad canónica en
  `.agents/skills/coasts/SKILL.md`. Cursor puede cargarla directamente; exponla a
  Claude Code mediante `.claude/skills/coasts/` si hace falta.

## Lecturas adicionales

- Lee la [guía de Harnesses](harnesses/README.md) para la matriz por harness
- Lee [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md) para el patrón de
  disposición compartida
- Lee la [documentación de Coastfiles](coastfiles/README.md) para aprender el
  esquema completo de configuración
- Aprende los comandos de la [CLI de Coast](concepts_and_terminology/CLI.md) para gestionar
  instancias
- Explora [Coastguard](concepts_and_terminology/COASTGUARD.md), la interfaz web para
  observar y controlar tus Coasts

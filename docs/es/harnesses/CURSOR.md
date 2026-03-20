# Cursor

[Cursor](https://cursor.com/docs/agent/overview) puede trabajar directamente en tu
checkout actual, y su función Parallel Agents también puede crear git
worktrees bajo `~/.cursor/worktrees/<project-name>/`.

Para la documentación sobre Coasts, eso significa que hay dos casos de configuración:

- si solo estás usando Cursor en el checkout actual, no se requiere ninguna entrada
  `worktree_dir` específica de Cursor
- si usas Cursor Parallel Agents, añade el directorio de worktrees de Cursor a
  `worktree_dir` para que Coasts pueda descubrir y asignar esos worktrees

## Configuración

### Solo checkout actual

Si Cursor solo está editando el checkout que ya abriste, Coasts no necesita
ninguna ruta de worktree especial específica de Cursor. Coasts tratará ese
checkout como cualquier otra raíz de repositorio local.

### Cursor Parallel Agents

Si usas Parallel Agents, añade `~/.cursor/worktrees/<project-name>` a
`worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

Cursor almacena el worktree de cada agente bajo ese directorio por proyecto. Coasts
expande `~` en tiempo de ejecución y trata la ruta como externa, por lo que las
instancias existentes deben recrearse para que el bind mount surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza inmediatamente después del cambio en el Coastfile, pero
asignar a un worktree de Cursor Parallel Agent requiere el bind mount externo
dentro del contenedor.

## Dónde va la guía de Coasts

### `AGENTS.md` o `.cursor/rules/coast.md`

Pon aquí las reglas breves y siempre activas de Coast Runtime:

- usa `AGENTS.md` si quieres las instrucciones de proyecto más portables
- usa `.cursor/rules/coast.md` si quieres reglas de proyecto nativas de Cursor y
  compatibilidad con la interfaz de configuración
- no dupliques el mismo bloque de Coast Runtime en ambos salvo que tengas una
  razón clara

### `.cursor/skills/coasts/SKILL.md` o `.agents/skills/coasts/SKILL.md` compartido

Pon aquí el flujo de trabajo reutilizable `/coasts`:

- para un repositorio solo de Cursor, `.cursor/skills/coasts/SKILL.md` es un lugar natural
- para un repositorio con múltiples harnesses, mantén la skill canónica en
  `.agents/skills/coasts/SKILL.md`; Cursor puede cargarla directamente
- la skill debe ser dueña del flujo de trabajo real de `/coasts`: `coast lookup`,
  `coast ls`, `coast run`, `coast assign`, `coast unassign`,
  `coast checkout` y `coast ui`

### `.cursor/commands/coasts.md`

Cursor también admite comandos de proyecto. Para la documentación sobre Coasts, trata los comandos como
opcionales:

- añade un comando solo cuando quieras un punto de entrada explícito `/coasts`
- una opción sencilla es hacer que el comando reutilice la misma skill
- si das al comando sus propias instrucciones separadas, estás asumiendo
  una segunda copia del flujo de trabajo para mantener

### `.cursor/worktrees.json`

Usa `.cursor/worktrees.json` para el bootstrap de worktrees propio de Cursor, no para la
política de Coasts:

- instalar dependencias
- copiar o enlazar simbólicamente archivos `.env`
- ejecutar migraciones de base de datos u otros pasos de bootstrap de una sola vez

No muevas las reglas de Coast Runtime ni el flujo de trabajo del Coast CLI a
`.cursor/worktrees.json`.

## Ejemplo de estructura

### Solo Cursor

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # opcional
.cursor/rules/coast.md            # alternativa opcional a AGENTS.md
.cursor/worktrees.json            # opcional, para bootstrap de Parallel Agents
```

### Cursor más otros harnesses

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # opcional
```

## Qué hace Coasts

- **Run** — `coast run <name>` crea una nueva instancia de Coast a partir de la compilación más reciente. Usa `coast run <name> -w <worktree>` para crear y asignar un worktree de Cursor en un solo paso. Consulta [Run](../concepts_and_terminology/RUN.md).
- **Checkout actual** — No se requiere ningún manejo especial de Cursor cuando Cursor está
  trabajando directamente en el repositorio que abriste.
- **Bind mount** — Para Parallel Agents, Coasts monta
  `~/.cursor/worktrees/<project-name>` en el contenedor en
  `/host-external-wt/{index}`.
- **Descubrimiento** — `git worktree list --porcelain` sigue teniendo alcance de repositorio, por lo que Coasts
  solo muestra los worktrees de Cursor que pertenecen al proyecto actual.
- **Nombres** — Los worktrees de Cursor Parallel Agent aparecen por sus nombres de rama en
  la CLI y la UI de Coasts.
- **Assign** — `coast assign` vuelve a montar `/workspace` desde la ruta del bind mount
  externo cuando se selecciona un worktree de Cursor.
- **Sincronización de ignorados por Git** — Sigue funcionando en el sistema de archivos del host con rutas
  absolutas.
- **Detección de huérfanos** — Si Cursor limpia worktrees antiguos, Coasts puede detectar
  el gitdir faltante y desasignarlos cuando sea necesario.

## Ejemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.cursor/worktrees/my-app"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.claude/worktrees/` — worktrees de Claude Code
- `~/.codex/worktrees/` — worktrees de Codex
- `~/.cursor/worktrees/my-app/` — worktrees de Cursor Parallel Agent

## Limitaciones

- Si no estás usando Cursor Parallel Agents, no añadas
  `~/.cursor/worktrees/<project-name>` solo porque casualmente estés editando en
  Cursor.
- Mantén las reglas de Coast Runtime en un único lugar siempre activo: `AGENTS.md` o
  `.cursor/rules/coast.md`. Duplicar ambos invita a la divergencia.
- Mantén el flujo de trabajo reutilizable `/coasts` en una skill. `.cursor/worktrees.json` es
  para el bootstrap de Cursor, no para la política de Coasts.
- Si un repositorio se comparte entre Cursor, Codex, Claude Code o T3 Code, prefiere
  la estructura compartida en [Multiple Harnesses](MULTIPLE_HARNESSES.md).

# Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) crea
worktrees dentro del proyecto en `.claude/worktrees/`. Debido a que ese directorio
vive dentro del repo, Coasts puede descubrir y asignar worktrees de Claude Code
sin ningún bind mount externo.

Claude Code también es el harness aquí con la división más clara entre tres
capas para Coasts:

- `CLAUDE.md` para reglas cortas, siempre activas, para trabajar con Coasts
- `.claude/skills/coasts/SKILL.md` para el flujo de trabajo reutilizable `/coasts`
- `.claude/commands/coasts.md` solo cuando quieras un archivo de comando como
  punto de entrada adicional

## Configuración

Agrega `.claude/worktrees` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

Debido a que `.claude/worktrees` es relativo al proyecto, no se necesita
ningún bind mount externo.

## Dónde va la guía de Coasts

### `CLAUDE.md`

Pon aquí las reglas para Coasts que deberían aplicarse en cada tarea. Mantén esto corto y
operativo:

- ejecutar `coast lookup` antes del primer comando de runtime en una sesión
- usar `coast exec` para pruebas, builds y comandos de servicio
- usar `coast ps` y `coast logs` para retroalimentación del runtime
- preguntar antes de crear o reasignar un Coast cuando no exista una coincidencia

### `.claude/skills/coasts/SKILL.md`

Pon aquí el flujo de trabajo reutilizable `/coasts`. Este es el lugar correcto para un flujo
que:

1. ejecuta `coast lookup` y reutiliza el Coast coincidente
2. recurre a `coast ls` cuando no hay coincidencia
3. ofrece `coast run`, `coast assign`, `coast unassign`, `coast checkout`, y
   `coast ui`
4. usa la CLI de Coast directamente como el contrato en lugar de envolverla

Si este repo también usa Codex, T3 Code, o Cursor, consulta
[Multiple Harnesses](MULTIPLE_HARNESSES.md) y mantén la skill canónica en
`.agents/skills/coasts/`, luego expónla a Claude Code.

### `.claude/commands/coasts.md`

Claude Code también admite archivos de comandos del proyecto. Para docs sobre Coasts, trata
esto como opcional:

- úsalo solo cuando específicamente quieras un archivo de comando
- una opción simple es hacer que el comando reutilice la misma skill
- si le das al comando sus propias instrucciones separadas, estás asumiendo una
  segunda copia del flujo de trabajo para mantener

## Estructura de ejemplo

### Solo Claude Code

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

Si este repo también usa Codex, T3 Code, o Cursor, usa el patrón compartido en
[Multiple Harnesses](MULTIPLE_HARNESSES.md) en lugar de duplicarlo aquí,
porque la guía específica duplicada por proveedor se vuelve más difícil de mantener sincronizada cada
vez que agregas otro harness.

## Lo que hace Coasts

- **Ejecutar** — `coast run <name>` crea una nueva instancia de Coast a partir del último build. Usa `coast run <name> -w <worktree>` para crear y asignar un worktree de Claude Code en un solo paso. Consulta [Run](../concepts_and_terminology/RUN.md).
- **Descubrimiento** — Coasts lee `.claude/worktrees` como cualquier otro directorio
  local de worktree.
- **Nomenclatura** — Los worktrees de Claude Code siguen el mismo comportamiento
  de nomenclatura de worktrees locales que otros worktrees dentro del repo en la UI y CLI de Coasts.
- **Asignar** — `coast assign` puede cambiar `/workspace` a un worktree de Claude Code
  sin ninguna indirección de bind-mount externo.
- **Sincronización de gitignored** — Funciona normalmente porque los worktrees viven dentro del
  árbol del repositorio.
- **Detección de huérfanos** — Si Claude Code elimina un worktree, Coasts puede detectar
  el gitdir faltante y desasignarlo cuando sea necesario.

## Ejemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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
- `~/.codex/worktrees/` — worktrees de Codex si también usas Codex en este repo

## Limitaciones

- Si duplicas el mismo flujo de trabajo `/coasts` entre `CLAUDE.md`,
  `.claude/skills`, y `.claude/commands`, esas copias divergirán. Mantén
  `CLAUDE.md` corto y mantén el flujo de trabajo reutilizable en una sola skill.
- Si quieres que un repo funcione limpiamente en múltiples harnesses, prefiere el patrón compartido
  en [Multiple Harnesses](MULTIPLE_HARNESSES.md).

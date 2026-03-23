# T3 Code

## Configuración rápida

Requiere la [Coast CLI](../GETTING_STARTED.md). Copia este prompt en el chat de tu
agente para configurar Coasts automáticamente:

```prompt-copy
t3_code_setup_prompt.txt
```

También puedes obtener el contenido de la skill desde la CLI: `coast skills-prompt`.

Después de la configuración, **reinicia T3 Code** para que los cambios en la skill y las reglas surtan efecto.

**Nota:** Es posible que T3 Code todavía no cargue skills a nivel de proyecto desde `.agents/skills/` o
`.claude/skills/`. El prompt de configuración también coloca la skill en
`~/.codex/skills/coasts/` para que esté disponible globalmente para el proveedor Codex.
Las reglas de Coast Runtime en `AGENTS.md` y `CLAUDE.md` siguen aplicándose en cada
tarea de todos modos.

---

[T3 Code](https://github.com/pingdotgg/t3code) crea git worktrees en
`~/.t3/worktrees/<project-name>/`, extraídos en ramas con nombre.

T3 Code envuelve a Codex, por lo que usa `AGENTS.md` para reglas siempre activas y
`.agents/skills/coasts/SKILL.md` para el flujo de trabajo reutilizable `/coasts`.

Debido a que estos worktrees viven fuera de la raíz del proyecto, Coasts necesita una configuración explícita para descubrirlos y montarlos.

## Setup

Agrega `~/.t3/worktrees/<project-name>` a `worktree_dir`. T3 Code anida los worktrees bajo un subdirectorio por proyecto, por lo que la ruta debe incluir el nombre del proyecto. En el ejemplo a continuación, `my-app` debe coincidir con el nombre real de la carpeta bajo `~/.t3/worktrees/` para tu repositorio.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coasts expande `~` en tiempo de ejecución y trata cualquier ruta que comience con `~/` o `/` como externa. Consulta [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para más detalles.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el bind mount surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza inmediatamente (Coasts lee el nuevo Coastfile), pero asignar a un worktree de T3 Code requiere el bind mount dentro del contenedor.

## Dónde va la guía de Coasts

Usa esta disposición para T3 Code:

- coloca las reglas breves de Coast Runtime en `AGENTS.md`
- coloca el flujo de trabajo reutilizable `/coasts` en `.agents/skills/coasts/SKILL.md`
- no agregues una capa separada de comando de proyecto o slash-command específica de T3 para
  Coasts
- si este repositorio usa múltiples harnesses, consulta
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) y
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

## Qué hace Coasts

- **Run** — `coast run <name>` crea una nueva instancia de Coast a partir de la compilación más reciente. Usa `coast run <name> -w <worktree>` para crear y asignar un worktree de T3 Code en un solo paso. Consulta [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — Al crear el contenedor, Coasts monta
  `~/.t3/worktrees/<project-name>` dentro del contenedor en
  `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` tiene alcance por repositorio, por lo que solo aparecen los worktrees que pertenecen al proyecto actual.
- **Naming** — Los worktrees de T3 Code usan ramas con nombre, por lo que aparecen por nombre de rama en la UI y la CLI de Coasts.
- **Assign** — `coast assign` vuelve a montar `/workspace` desde la ruta externa del bind mount.
- **Gitignored sync** — Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el bind mount.
- **Orphan detection** — El watcher de git escanea directorios externos de forma recursiva, filtrando por punteros gitdir de `.git`. Si T3 Code elimina un espacio de trabajo, Coasts desasigna automáticamente la instancia.

## Ejemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
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

- `.claude/worktrees/` — Claude Code (local, sin manejo especial)
- `~/.codex/worktrees/` — Codex (externo, con bind mount)
- `~/.t3/worktrees/my-app/` — T3 Code (externo, con bind mount; reemplaza `my-app` con el nombre de la carpeta de tu repositorio)

## Limitaciones

- Evita depender de variables de entorno específicas de T3 Code para la configuración en tiempo de ejecución dentro de Coasts. Coasts gestiona puertos, rutas de espacio de trabajo y descubrimiento de servicios de forma independiente — usa `[ports]` en Coastfile y `coast exec` en su lugar.

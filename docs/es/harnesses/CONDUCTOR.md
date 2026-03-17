# Conductor

[Conductor](https://conductor.build/) ejecuta agentes paralelos de Claude Code, cada uno en su propio espacio de trabajo aislado. Los espacios de trabajo son `git worktrees` almacenados en `~/conductor/workspaces/<project-name>/`. Cada espacio de trabajo se extrae en una rama con nombre.

Debido a que estos worktrees viven fuera de la raíz del proyecto, Coast necesita una configuración explícita para descubrirlos y montarlos.

## Configuración

Agrega `~/conductor/workspaces/<project-name>` a `worktree_dir`. A diferencia de Codex (que almacena todos los proyectos bajo un único directorio plano), Conductor anida los worktrees bajo un subdirectorio por proyecto, por lo que la ruta debe incluir el nombre del proyecto:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor te permite configurar la ruta de los espacios de trabajo por repositorio, por lo que el valor predeterminado `~/conductor/workspaces` puede no coincidir con tu configuración. Revisa la configuración de tu repositorio de Conductor para encontrar la ruta real y ajústala en consecuencia — el principio es el mismo sin importar dónde se encuentre el directorio.

Coast expande `~` en tiempo de ejecución y trata cualquier ruta que comience con `~/` o `/` como externa. Consulta [Directorios de Worktree](../coastfiles/WORKTREE_DIR.md) para más detalles.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el montaje bind surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza de inmediato (Coast lee el nuevo Coastfile), pero asignar a un worktree de Conductor requiere el montaje bind dentro del contenedor.

## Qué hace Coast

- **Montaje bind** — Al crear el contenedor, Coast monta `~/conductor/workspaces/<project-name>` dentro del contenedor en `/host-external-wt/{index}`.
- **Descubrimiento** — `git worktree list --porcelain` tiene alcance de repositorio, por lo que solo aparecen los worktrees que pertenecen al proyecto actual.
- **Nombres** — Los worktrees de Conductor usan ramas con nombre, por lo que aparecen por nombre de rama en la interfaz y la CLI de Coast (por ejemplo, `scroll-to-bottom-btn`). Una rama solo puede estar extraída en un espacio de trabajo de Conductor a la vez.
- **Asignación** — `coast assign` vuelve a montar `/workspace` desde la ruta de montaje bind externa.
- **Sincronización de gitignored** — Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el montaje bind.
- **Detección de huérfanos** — El observador de git escanea directorios externos de forma recursiva, filtrando por punteros `gitdir` de `.git`. Si Conductor archiva o elimina un espacio de trabajo, Coast desasigna automáticamente la instancia.

## Ejemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/conductor/workspaces/my-app"]
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

- `.worktrees/` — Worktrees gestionados por Coast
- `.claude/worktrees/` — Claude Code (local, sin manejo especial)
- `~/.codex/worktrees/` — Codex (externo, montado con bind)
- `~/conductor/workspaces/my-app/` — Conductor (externo, montado con bind)

## Variables de entorno de Conductor

- Evita depender de variables de entorno específicas de Conductor (p. ej., `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) para la configuración en tiempo de ejecución dentro de Coasts. Coast administra puertos, rutas de espacios de trabajo y descubrimiento de servicios de forma independiente — usa `[ports]` del Coastfile y `coast exec` en su lugar.

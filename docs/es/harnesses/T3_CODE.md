# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) es un arnés de agente de programación de código abierto de Ping. Cada espacio de trabajo es un git worktree almacenado en `~/.t3/worktrees/<project-name>/`, extraído en una rama con nombre.

Debido a que estos worktrees viven fuera de la raíz del proyecto, Coast necesita una configuración explícita para descubrirlos y montarlos.

## Configuración

Agrega `~/.t3/worktrees/<project-name>` a `worktree_dir`. T3 Code anida los worktrees bajo un subdirectorio por proyecto, por lo que la ruta debe incluir el nombre del proyecto. En el ejemplo a continuación, `my-app` debe coincidir con el nombre real de la carpeta bajo `~/.t3/worktrees/` para tu repositorio.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coast expande `~` en tiempo de ejecución y trata cualquier ruta que comience con `~/` o `/` como externa. Consulta [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para más detalles.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el bind mount surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza inmediatamente (Coast lee el nuevo Coastfile), pero asignar a un worktree de T3 Code requiere el bind mount dentro del contenedor.

## Qué hace Coast

- **Bind mount** — Al crear el contenedor, Coast monta `~/.t3/worktrees/<project-name>` dentro del contenedor en `/host-external-wt/{index}`.
- **Descubrimiento** — `git worktree list --porcelain` tiene alcance por repositorio, por lo que solo aparecen los worktrees que pertenecen al proyecto actual.
- **Nombres** — Los worktrees de T3 Code usan ramas con nombre, por lo que aparecen por nombre de rama en la UI y la CLI de Coast.
- **Asignar** — `coast assign` vuelve a montar `/workspace` desde la ruta externa del bind mount.
- **Sincronización de gitignored** — Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el bind mount.
- **Detección de huérfanos** — El watcher de git escanea directorios externos de forma recursiva, filtrando por punteros gitdir de `.git`. Si T3 Code elimina un espacio de trabajo, Coast desasigna automáticamente la instancia.

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

- `.worktrees/` — Worktrees gestionados por Coast
- `.claude/worktrees/` — Claude Code (local, sin manejo especial)
- `~/.codex/worktrees/` — Codex (externo, con bind mount)
- `~/.t3/worktrees/my-app/` — T3 Code (externo, con bind mount; reemplaza `my-app` con el nombre de la carpeta de tu repositorio)

## Limitaciones

- Coast descubre y monta worktrees de T3 Code, pero no los crea ni los elimina.
- Los nuevos worktrees creados por `coast assign` siempre van en el `default_worktree_dir` local, nunca en un directorio externo.
- Evita depender de variables de entorno específicas de T3 Code para la configuración en tiempo de ejecución dentro de Coasts. Coast gestiona puertos, rutas de espacio de trabajo y descubrimiento de servicios de forma independiente — usa `[ports]` en Coastfile y `coast exec` en su lugar.

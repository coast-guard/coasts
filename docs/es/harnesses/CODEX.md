# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) crea worktrees en `$CODEX_HOME/worktrees` (normalmente `~/.codex/worktrees`). Cada worktree vive bajo un directorio con hash opaco como `~/.codex/worktrees/a0db/project-name`, comienza en un HEAD desacoplado y se limpia automáticamente según la política de retención de Codex.

De la [documentación de Codex](https://developers.openai.com/codex/app/worktrees/):

> ¿Puedo controlar dónde se crean los worktrees?
> No por ahora. Codex crea worktrees en `$CODEX_HOME/worktrees` para poder administrarlos de forma consistente.

Debido a que estos worktrees viven fuera de la raíz del proyecto, Coast necesita una configuración explícita para descubrirlos y montarlos.

## Configuración

Agrega `~/.codex/worktrees` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coast expande `~` en tiempo de ejecución y trata cualquier ruta que comience con `~/` o `/` como externa. Consulta [Directorios de Worktree](../coastfiles/WORKTREE_DIR.md) para más detalles.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el bind mount surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza de inmediato (Coast lee el nuevo Coastfile), pero asignar a un worktree de Codex requiere el bind mount dentro del contenedor.

## Qué hace Coast

- **Bind mount** -- Al crear el contenedor, Coast monta `~/.codex/worktrees` dentro del contenedor en `/host-external-wt/{index}`.
- **Descubrimiento** -- `git worktree list --porcelain` está limitado al repositorio, por lo que solo aparecen los worktrees de Codex que pertenecen al proyecto actual, aunque el directorio contenga worktrees de muchos proyectos.
- **Nomenclatura** -- Los worktrees con HEAD desacoplado se muestran como su ruta relativa dentro del directorio externo (`a0db/my-app`, `eca7/my-app`). Los worktrees basados en ramas muestran el nombre de la rama.
- **Asignación** -- `coast assign` vuelve a montar `/workspace` desde la ruta del bind mount externo.
- **Sincronización de archivos ignorados por Git** -- Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el bind mount.
- **Detección de huérfanos** -- El watcher de git escanea directorios externos recursivamente, filtrando por punteros gitdir de `.git`. Si Codex elimina un worktree, Coast desasigna automáticamente la instancia.

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

- `.worktrees/` -- Worktrees administrados por Coast
- `.claude/worktrees/` -- Claude Code (local, sin manejo especial)
- `~/.codex/worktrees/` -- Codex (externo, montado con bind)

## Limitaciones

- Coast descubre y monta worktrees de Codex, pero no los crea ni los elimina.
- Codex puede limpiar worktrees en cualquier momento. La detección de huérfanos de Coast maneja esto correctamente.
- Los nuevos worktrees creados por `coast assign` siempre van al `default_worktree_dir` local, nunca a un directorio externo.

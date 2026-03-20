# Conductor

[Conductor](https://conductor.build/) ejecuta agentes paralelos de Claude Code, cada uno en su propio espacio de trabajo aislado. Los espacios de trabajo son `git worktrees` almacenados en `~/conductor/workspaces/<project-name>/`. Cada espacio de trabajo se extrae en una rama con nombre.

Debido a que estos worktrees viven fuera de la raíz del proyecto, Coasts necesita una configuración explícita para descubrirlos y montarlos.

## Configuración

Agrega `~/conductor/workspaces/<project-name>` a `worktree_dir`. A diferencia de Codex (que almacena todos los proyectos bajo un único directorio plano), Conductor anida los worktrees bajo un subdirectorio por proyecto, por lo que la ruta debe incluir el nombre del proyecto. En el ejemplo de abajo, `my-app` debe coincidir con el nombre real de la carpeta bajo `~/conductor/workspaces/` para tu repositorio.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor te permite configurar la ruta de los espacios de trabajo por repositorio, por lo que el valor predeterminado `~/conductor/workspaces` puede no coincidir con tu configuración. Revisa la configuración de tu repositorio de Conductor para encontrar la ruta real y ajústala en consecuencia — el principio es el mismo sin importar dónde se encuentre el directorio.

Coasts expande `~` en tiempo de ejecución y trata cualquier ruta que comience con `~/` o `/` como externa. Consulta [Directorios de Worktree](../coastfiles/WORKTREE_DIR.md) para
más detalles.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el montaje bind surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza de inmediato (Coasts lee el nuevo Coastfile), pero
asignar a un worktree de Conductor requiere el montaje bind dentro del contenedor.

## Dónde va la guía de Coasts

Trata a Conductor como su propio harness para trabajar con Coasts:

- pon las reglas cortas de Coast Runtime en `CLAUDE.md`
- usa scripts de Configuración del Repositorio de Conductor para comportamiento
  de configuración o ejecución que sea realmente específico de Conductor
- no asumas aquí el comportamiento completo de comandos de proyecto o skills de
  proyecto de Claude Code
- si agregas un comando y no aparece, cierra y vuelve a abrir por completo
  Conductor antes de volver a probar
- si este repositorio también usa otros harnesses, consulta
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) y
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) para formas de mantener
  el flujo de trabajo compartido de `/coasts` en un solo lugar

## Qué hace Coasts

- **Run** — `coast run <name>` crea una nueva instancia de Coast a partir de la compilación más reciente. Usa `coast run <name> -w <worktree>` para crear y asignar un worktree de Conductor en un solo paso. Consulta [Run](../concepts_and_terminology/RUN.md).
- **Montaje bind** — Al crear el contenedor, Coasts monta
  `~/conductor/workspaces/<project-name>` dentro del contenedor en
  `/host-external-wt/{index}`.
- **Descubrimiento** — `git worktree list --porcelain` tiene alcance de repositorio, por lo que solo aparecen los worktrees que pertenecen al proyecto actual.
- **Nombres** — Los worktrees de Conductor usan ramas con nombre, por lo que aparecen por nombre de rama en la interfaz y la CLI de Coasts (por ejemplo, `scroll-to-bottom-btn`). Una rama solo puede estar extraída en un espacio de trabajo de Conductor a la vez.
- **Asignación** — `coast assign` vuelve a montar `/workspace` desde la ruta de montaje bind externa.
- **Sincronización de gitignored** — Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el montaje bind.
- **Detección de huérfanos** — El observador de git escanea directorios externos
  de forma recursiva, filtrando por punteros `gitdir` de `.git`. Si Conductor
  archiva o elimina un espacio de trabajo, Coasts desasigna automáticamente la instancia.

## Ejemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = ["~/conductor/workspaces/my-app"]
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

- `~/conductor/workspaces/my-app/` — Conductor (externo, montado con bind; reemplaza `my-app` por el nombre de la carpeta de tu repositorio)

## Variables de entorno de Conductor

- Evita depender de variables de entorno específicas de Conductor (p. ej.,
  `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) para la configuración en tiempo de ejecución
  dentro de Coasts. Coasts administra puertos, rutas de espacios de trabajo y descubrimiento de servicios
  de forma independiente — usa `[ports]` del Coastfile y `coast exec` en su lugar.

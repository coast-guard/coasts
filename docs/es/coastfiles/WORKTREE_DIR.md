# Directorios de worktree

El campo `worktree_dir` en `[coast]` controla dónde viven los git worktrees. Coast usa git worktrees para dar a cada instancia su propia copia del código base en una rama diferente, sin duplicar el repositorio completo.

## Sintaxis

`worktree_dir` acepta una sola cadena o un arreglo de cadenas:

```toml
# Single directory (default)
worktree_dir = ".worktrees"

# Multiple directories
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

Cuando se omite, el valor predeterminado es `".worktrees"`.

## Tipos de ruta

### Rutas relativas

Las rutas que no comienzan con `~/` o `/` se resuelven en relación con la raíz del proyecto. Estas son las más comunes y no requieren manejo especial — están dentro del directorio del proyecto y están disponibles automáticamente dentro del contenedor de Coast mediante el bind mount estándar `/host-project`.

```toml
worktree_dir = ".worktrees"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### Rutas con tilde (externas)

Las rutas que comienzan con `~/` se expanden al directorio home del usuario y se tratan como directorios de worktree **externos**. Coast añade un bind mount separado para que el contenedor pueda acceder a ellos.

```toml
worktree_dir = ["~/.codex/worktrees", ".worktrees"]
```

Así es como se integra con herramientas que crean worktrees fuera de la raíz de su proyecto, como OpenAI Codex (que siempre crea worktrees en `$CODEX_HOME/worktrees`).

### Rutas absolutas (externas)

Las rutas que comienzan con `/` también se tratan como externas y reciben su propio bind mount.

```toml
worktree_dir = ["/shared/worktrees", ".worktrees"]
```

## Cómo funcionan los directorios externos

Cuando Coast encuentra un directorio de worktree externo (ruta con tilde o absoluta), ocurren tres cosas:

1. **Bind mount del contenedor** — En el momento de la creación del contenedor (`coast run`), la ruta resuelta del host se monta mediante bind en el contenedor en `/host-external-wt/{index}`, donde `{index}` es la posición en el arreglo `worktree_dir`. Esto hace que los archivos externos sean accesibles dentro del contenedor.

2. **Filtrado de proyecto** — Los directorios externos pueden contener worktrees de múltiples proyectos. Coast usa `git worktree list --porcelain` (que está inherentemente limitado al repositorio actual) para descubrir solo los worktrees que pertenecen a este proyecto. El watcher de git también verifica la pertenencia leyendo el archivo `.git` de cada worktree y comprobando que su puntero `gitdir:` se resuelva de vuelta al repositorio actual.

3. **Remontaje del workspace** — Cuando hace `coast assign` a un worktree externo, Coast vuelve a montar `/workspace` desde la ruta del bind mount externo en lugar de la ruta habitual `/host-project/{dir}/{name}`.

## Nombres de los worktrees externos

Los worktrees externos con una rama checked out aparecen por el nombre de su rama, igual que los worktrees locales.

Los worktrees externos en un **detached HEAD** (común con Codex) aparecen usando su ruta relativa dentro del directorio externo. Por ejemplo, un worktree de Codex en `~/.codex/worktrees/a0db/coastguard-platform` aparece como `a0db/coastguard-platform` en la UI y la CLI.

## `default_worktree_dir`

Controla qué directorio se usa cuando Coast crea un worktree **nuevo** (por ejemplo, cuando asigna una rama que no tiene un worktree existente). El valor predeterminado es la primera entrada en `worktree_dir`.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

Los directorios externos nunca se usan para crear worktrees nuevos — Coast siempre crea worktrees en un directorio local (relativo). El campo `default_worktree_dir` solo es necesario cuando quiere sobrescribir el valor predeterminado (la primera entrada).

## Ejemplos

### Integración con Codex

OpenAI Codex crea worktrees en `~/.codex/worktrees/{hash}/{project-name}`. Para hacer que estos sean visibles y asignables en Coast:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Después de agregar esto, los worktrees de Codex aparecen en el modal de checkout y en la salida de `coast ls`. Puede asignar una instancia de Coast a un worktree de Codex para ejecutar su código en un entorno de desarrollo completo.

Nota: el contenedor debe recrearse (`coast run`) después de agregar un directorio externo para que el bind mount surta efecto. Reiniciar una instancia existente no es suficiente.

### Integración con Claude Code

Claude Code crea worktrees dentro del proyecto en `.claude/worktrees/`. Como esta es una ruta relativa (dentro de la raíz del proyecto), funciona como cualquier otro directorio de worktree local — no se necesita ningún montaje externo:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### Los tres juntos

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

## Lectura en vivo del Coastfile

Los cambios en `worktree_dir` en su Coastfile surten efecto inmediatamente para el **listado** de worktrees (la API y el watcher de git leen el Coastfile en vivo desde el disco, no solo el artefacto de compilación en caché). Sin embargo, los **bind mounts** externos solo se crean en el momento de la creación del contenedor, por lo que necesita recrear la instancia para que un directorio externo recién agregado pueda montarse.

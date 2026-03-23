# Shep

## Configuración rápida

Requiere el [Coast CLI](../GETTING_STARTED.md). Copia este prompt en el chat de tu
agente para configurar Coasts automáticamente:

```prompt-copy
shep_setup_prompt.txt
```

También puedes obtener el contenido de la skill desde el CLI: `coast skills-prompt`.

Después de la configuración, **cierra y vuelve a abrir tu editor** para que la nueva skill y las
instrucciones del proyecto surtan efecto.

---

[Shep](https://shep-ai.github.io/cli/) crea worktrees en `~/.shep/repos/{hash}/wt/{branch-slug}`. El hash son los primeros 16 caracteres hexadecimales del SHA-256 de la ruta absoluta del repositorio, por lo que es determinista por repositorio pero opaco. Todos los worktrees de un repositorio dado comparten el mismo hash y se diferencian por el subdirectorio `wt/{branch-slug}`.

Desde el Shep CLI, `shep feat show <feature-id>` imprime la ruta del worktree, o
`ls ~/.shep/repos` lista los directorios hash por repositorio.

Debido a que el hash varía según el repositorio, Coasts usa un **patrón glob** para descubrir
los worktrees de shep sin requerir que el usuario codifique el hash manualmente.

## Configuración

Agrega `~/.shep/repos/*/wt` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

El `*` coincide con el directorio hash por repositorio. En tiempo de ejecución Coasts expande el glob,
encuentra el directorio coincidente (p. ej. `~/.shep/repos/a21f0cda9ab9d456/wt`), y
lo monta mediante bind mount dentro del contenedor. Consulta
[Directorios de Worktree](../coastfiles/WORKTREE_DIR.md) para ver todos los detalles sobre los
patrones glob.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el bind mount surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza inmediatamente (Coasts lee el nuevo Coastfile), pero
asignar a un worktree de Shep requiere el bind mount dentro del contenedor.

## Dónde va la guía de Coasts

Shep envuelve Claude Code internamente, así que sigue las convenciones de Claude Code:

- coloca las reglas cortas de Coast Runtime en `CLAUDE.md`
- coloca el flujo de trabajo reutilizable `/coasts` en `.claude/skills/coasts/SKILL.md` o
  en el compartido `.agents/skills/coasts/SKILL.md`
- si este repositorio también usa otros harnesses, consulta
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) y
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)

## Qué hace Coasts

- **Run** -- `coast run <name>` crea una nueva instancia de Coast a partir de la compilación más reciente. Usa `coast run <name> -w <worktree>` para crear y asignar un worktree de Shep en un solo paso. Consulta [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- Al crear el contenedor, Coasts resuelve el glob
  `~/.shep/repos/*/wt` y monta cada directorio coincidente dentro del contenedor en
  `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` tiene alcance de repositorio, por lo que solo
  aparecen los worktrees que pertenecen al proyecto actual.
- **Naming** -- Los worktrees de Shep usan ramas con nombre, por lo que aparecen por nombre
  de rama en la UI y el CLI de Coasts (p. ej., `feat-green-background`).
- **Assign** -- `coast assign` vuelve a montar `/workspace` desde la ruta de bind mount externa.
- **Gitignored sync** -- Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el bind mount.
- **Orphan detection** -- El observador de git escanea directorios externos
  recursivamente, filtrando por punteros gitdir de `.git`. Si Shep elimina un
  worktree, Coasts desasigna automáticamente la instancia.

## Ejemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
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

- `~/.shep/repos/*/wt` -- Shep (externo, montado mediante bind mount vía expansión de glob)

## Estructura de rutas de Shep

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

Puntos clave:
- Mismo repositorio = mismo hash siempre (determinista, no aleatorio)
- Repositorios diferentes = hashes diferentes
- Los separadores de ruta se normalizan a `/` antes de aplicar hash
- El hash se puede encontrar mediante `shep feat show <feature-id>` o `ls ~/.shep/repos`

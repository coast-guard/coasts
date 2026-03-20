# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) crea worktrees en `$CODEX_HOME/worktrees` (normalmente `~/.codex/worktrees`). Cada worktree vive bajo un directorio con hash opaco como `~/.codex/worktrees/a0db/project-name`, comienza en un HEAD desacoplado y se limpia automáticamente según la política de retención de Codex.

De la [documentación de Codex](https://developers.openai.com/codex/app/worktrees/):

> ¿Puedo controlar dónde se crean los worktrees?
> No por ahora. Codex crea worktrees en `$CODEX_HOME/worktrees` para poder administrarlos de forma consistente.

Debido a que estos worktrees viven fuera de la raíz del proyecto, Coasts necesita una configuración explícita para descubrirlos y montarlos.

## Configuración

Agrega `~/.codex/worktrees` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coasts expande `~` en tiempo de ejecución y trata cualquier ruta que comience con `~/` o `/` como externa. Consulta [Directorios de Worktree](../coastfiles/WORKTREE_DIR.md) para más detalles.

Después de cambiar `worktree_dir`, las instancias existentes deben **recrearse** para que el bind mount surta efecto:

```bash
coast rm my-instance
coast build
coast run my-instance
```

La lista de worktrees se actualiza de inmediato (Coasts lee el nuevo Coastfile), pero asignar a un worktree de Codex requiere el bind mount dentro del contenedor.

## Dónde va la guía de Coasts

Usa el archivo de instrucciones del proyecto de Codex y la disposición compartida de skills para trabajar con Coasts:

- coloca las reglas cortas de Coast Runtime en `AGENTS.md`
- coloca el flujo reutilizable de `/coasts` en `.agents/skills/coasts/SKILL.md`
- Codex expone esa skill como el comando `/coasts`
- si usas metadatos específicos de Codex, mantenlos junto a la skill en
  `.agents/skills/coasts/agents/openai.yaml`
- no crees un archivo de comandos del proyecto separado solo para documentación sobre Coasts; la skill es la superficie reutilizable
- si este repositorio también usa Cursor o Claude Code, mantén la skill canónica en
  `.agents/skills/` y expónla desde allí. Consulta
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) y
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

Por ejemplo, un `.agents/skills/coasts/agents/openai.yaml` mínimo podría verse
así:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

Eso mantiene la skill visible en Codex con una etiqueta más agradable y hace que `/coasts` sea un comando explícito. Solo agrega `dependencies.tools` si la skill también necesita servidores MCP u otra integración de herramientas administrada por OpenAI.

## Qué hace Coasts

- **Run** -- `coast run <name>` crea una nueva instancia de Coast a partir de la compilación más reciente. Usa `coast run <name> -w <worktree>` para crear y asignar un worktree de Codex en un solo paso. Consulta [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- Al crear el contenedor, Coasts monta
  `~/.codex/worktrees` dentro del contenedor en `/host-external-wt/{index}`.
- **Descubrimiento** -- `git worktree list --porcelain` está limitado al repositorio, por lo que solo aparecen los worktrees de Codex que pertenecen al proyecto actual, aunque el directorio contenga worktrees de muchos proyectos.
- **Nomenclatura** -- Los worktrees con HEAD desacoplado se muestran como su ruta relativa dentro del directorio externo (`a0db/my-app`, `eca7/my-app`). Los worktrees basados en ramas muestran el nombre de la rama.
- **Asignación** -- `coast assign` vuelve a montar `/workspace` desde la ruta del bind mount externo.
- **Sincronización de archivos ignorados por Git** -- Se ejecuta en el sistema de archivos del host con rutas absolutas, funciona sin el bind mount.
- **Detección de huérfanos** -- El watcher de git escanea directorios externos
  recursivamente, filtrando por punteros gitdir de `.git`. Si Codex elimina un
  worktree, Coasts desasigna automáticamente la instancia.

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

- `.claude/worktrees/` -- Claude Code (local, sin manejo especial)
- `~/.codex/worktrees/` -- Codex (externo, montado con bind)

## Limitaciones

- Codex puede limpiar worktrees en cualquier momento. La detección de huérfanos en Coasts maneja esto correctamente.

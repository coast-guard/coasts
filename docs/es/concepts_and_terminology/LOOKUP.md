# Búsqueda

`coast lookup` descubre qué instancias de Coast se están ejecutando para el directorio de trabajo actual del llamador. Es el primer comando que un agente del lado del host debería ejecutar para orientarse: "Estoy editando código aquí, ¿con qué Coast(s) debería interactuar?"

```bash
coast lookup
```

Lookup detecta si estás dentro de un [worktree](ASSIGN.md) o en la raíz del proyecto, consulta al daemon por instancias coincidentes e imprime los resultados con puertos, URLs y comandos de ejemplo.

## Por Qué Existe

Un agente de programación con IA que se ejecuta en el host (Cursor, Claude Code, Codex, etc.) edita archivos a través del [sistema de archivos compartido](FILESYSTEM.md) y llama comandos de Coast CLI para operaciones en tiempo de ejecución. Pero primero el agente necesita responder una pregunta básica: **¿qué instancia de Coast corresponde al directorio en el que estoy trabajando?**

Sin `coast lookup`, el agente tendría que ejecutar `coast ls`, analizar la tabla completa de instancias, averiguar en qué worktree está y cruzar referencias. `coast lookup` hace todo eso en un solo paso y devuelve una salida estructurada que los agentes pueden consumir directamente.

Este comando debería incluirse en cualquier archivo SKILL.md, AGENTS.md o de reglas de nivel superior para flujos de trabajo de agentes que usen Coast. Es el punto de entrada para que un agente descubra su contexto de ejecución.

## Modos de Salida

### Predeterminado (legible para humanos)

```bash
coast lookup
```

```text
Coast instances for worktree feature/oauth (my-app):

  dev-1  running  ★ checked out

  Primary URL:  http://dev-1.localhost:62217

  SERVICE              CANONICAL       DYNAMIC
  ★ web                3000            62217
    api                8080            63889
    postgres           5432            55681

  Examples (exec starts at the workspace root where your Coastfile is, cd to your target directory first):
    coast exec dev-1 -- sh -c "cd <dir> && <command>"
    coast logs dev-1 --service <service>
    coast ps dev-1
```

La sección de ejemplos recuerda a los agentes (y humanos) que `coast exec` inicia en la raíz del workspace — el directorio donde vive el Coastfile. Para ejecutar un comando en un subdirectorio, haz `cd` a él dentro del exec.

### Compacto (`--compact`)

Devuelve un array JSON de nombres de instancia. Diseñado para scripts y herramientas de agentes que solo necesitan saber a qué instancias dirigirse.

```bash
coast lookup --compact
```

```text
["dev-1"]
```

Múltiples instancias en el mismo worktree:

```text
["dev-1","dev-2"]
```

Sin coincidencias:

```text
[]
```

### JSON (`--json`)

Devuelve la respuesta estructurada completa como JSON con impresión bonita. Diseñado para agentes que necesitan puertos, URLs y estado en un formato legible por máquina.

```bash
coast lookup --json
```

```json
{
  "project": "my-app",
  "worktree": "feature/oauth",
  "project_root": "/Users/dev/my-app",
  "instances": [
    {
      "name": "dev-1",
      "status": "Running",
      "checked_out": true,
      "branch": "feature/oauth",
      "primary_url": "http://dev-1.localhost:62217",
      "ports": [
        { "logical_name": "web", "canonical_port": 3000, "dynamic_port": 62217, "is_primary": true },
        { "logical_name": "api", "canonical_port": 8080, "dynamic_port": 63889, "is_primary": false }
      ]
    }
  ]
}
```

## Cómo Lo Resuelve

Lookup recorre hacia arriba desde el directorio de trabajo actual para encontrar el Coastfile más cercano y luego determina en qué worktree estás:

1. Si tu cwd está bajo `{project_root}/{worktree_dir}/{name}/...`, lookup encuentra instancias asignadas a ese worktree.
2. Si tu cwd es la raíz del proyecto (o cualquier directorio que no esté dentro de un worktree), lookup encuentra instancias **sin worktree asignado** — aquellas que aún apuntan a la raíz del proyecto.

Esto significa que lookup también funciona desde subdirectorios. Si estás en `my-app/.coasts/feature-oauth/src/api/`, lookup aún resuelve `feature-oauth` como el worktree.

## Códigos de Salida

| Code | Meaning |
|------|---------|
| 0 | Se encontró una o más instancias coincidentes |
| 1 | No hay instancias coincidentes (resultado vacío) |

Esto hace que lookup sea utilizable en condicionales de shell:

```bash
if coast lookup > /dev/null 2>&1; then
  coast exec dev-1 -- sh -c "cd src && npm test"
fi
```

## Para Flujos de Trabajo de Agentes

El patrón típico de integración de agentes:

1. El agente comienza a trabajar en un directorio de worktree.
2. El agente ejecuta `coast lookup` para descubrir nombres de instancia, puertos, URLs y comandos de ejemplo.
3. El agente usa el nombre de la instancia para todos los comandos posteriores de Coast: `coast exec`, `coast logs`, `coast ps`.

```text
┌─── Agent (host machine) ────────────────────────────┐
│                                                      │
│  1. coast lookup                                     │
│       → instance names, ports, URLs, examples        │
│  2. coast exec dev-1 -- sh -c "cd src && npm test"   │
│  3. coast logs dev-1 --service web --tail 50         │
│  4. coast ps dev-1                                   │
│                                                      │
└──────────────────────────────────────────────────────┘
```

Si el agente está trabajando a través de múltiples worktrees, ejecuta `coast lookup` desde cada directorio de worktree para resolver la instancia correcta para cada contexto.

Consulta también [Filesystem](FILESYSTEM.md) para ver cómo los agentes del host interactúan con Coast, [Assign and Unassign](ASSIGN.md) para conceptos de worktree, y [Exec & Docker](EXEC_AND_DOCKER.md) para ejecutar comandos dentro de un Coast.

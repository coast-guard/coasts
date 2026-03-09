# Asignar y Desasignar

Asignar y desasignar controlan a qué worktree está apuntando una instancia de Coast. Consulta [Filesystem](FILESYSTEM.md) para ver cómo funciona el cambio de worktree a nivel de montaje.

## Asignar

`coast assign` cambia una instancia de Coast a un worktree específico. Coast crea el worktree si aún no existe, actualiza el código dentro de Coast y reinicia los servicios según la estrategia de asignación configurada.

```bash
coast assign dev-1 --worktree feature/oauth
```

```text
Before:
┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘

coast assign dev-1 --worktree feature/oauth

After:
┌─── dev-1 ──────────────────┐
│  branch: feature/oauth     │
│  worktree: feature/oauth   │
│                            │
│  postgres → skipped (none) │
│  web      → hot swapped    │
│  api      → restarted      │
│  worker   → rebuilt        │
└────────────────────────────┘
```

Después de asignar, `dev-1` está ejecutando la rama `feature/oauth` con todos sus servicios activos.

## Desasignar

`coast unassign` cambia una instancia de Coast de vuelta a la raíz del proyecto (tu rama main/master). Se elimina la asociación con el worktree y Coast vuelve a ejecutarse desde el repositorio principal.

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## Estrategias de asignación

Cuando se asigna una instancia de Coast a un nuevo worktree, cada servicio necesita saber cómo manejar el cambio de código. Esto se configura por servicio en tu [Coastfile](COASTFILE_TYPES.md) bajo `[assign]`:

```toml
[assign]
default = "restart"

[assign.services]
postgres = "none"
redis = "none"
web = "hot"
worker = "rebuild"
```

```text
coast assign dev-1 --worktree feature/billing

  postgres (strategy: none)    →  skipped, unchanged between branches
  redis (strategy: none)       →  skipped, unchanged between branches
  web (strategy: hot)          →  filesystem swapped, file watcher picks it up
  api (strategy: restart)      →  container restarted
  worker (strategy: rebuild)   →  image rebuilt, container restarted
```

Las estrategias disponibles son:

- **none** — no hacer nada. Úsalo para servicios que no cambian entre ramas, como Postgres o Redis.
- **hot** — intercambiar únicamente el sistema de archivos. El servicio sigue ejecutándose y recoge los cambios mediante la propagación de montajes y los observadores de archivos (p. ej., un servidor de desarrollo con recarga en caliente).
- **restart** — reiniciar el contenedor del servicio. Úsalo para servicios interpretados que solo necesitan un reinicio del proceso. Esta es la opción predeterminada.
- **rebuild** — reconstruir la imagen del servicio y reiniciar. Úsalo cuando el cambio de rama afecta al `Dockerfile` o a las dependencias de tiempo de compilación.

También puedes especificar disparadores de reconstrucción para que un servicio solo se reconstruya cuando cambien archivos específicos:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

Si ninguno de los archivos disparadores cambió entre ramas, el servicio omite la reconstrucción incluso si la estrategia está configurada como `rebuild`.

## Worktrees eliminados

Si se elimina un worktree asignado, el daemon `coastd` desasigna automáticamente esa instancia de vuelta a la raíz del repositorio principal de Git.

---

> **Consejo: Reducir la latencia de asignación en bases de código grandes**
>
> Internamente, la primera asignación a un nuevo worktree inicializa determinados archivos ignorados por git en ese worktree, y los servicios con `[assign.rebuild_triggers]` pueden ejecutar `git diff --name-only` para decidir si es necesaria una reconstrucción. En bases de código grandes, ese paso de inicialización y las reconstrucciones innecesarias tienden a dominar el tiempo de asignación.
>
> Usa `exclude_paths` en tu Coastfile para reducir la superficie de inicialización de archivos ignorados por git, usa `"hot"` para servicios con observadores de archivos y mantén `[assign.rebuild_triggers]` centrado en verdaderas entradas de tiempo de compilación. Si necesitas refrescar manualmente la inicialización de archivos ignorados para un worktree existente, ejecuta `coast assign --force-sync`. Consulta [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) para una guía completa.

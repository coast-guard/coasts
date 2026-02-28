# Asignar y Desasignar

Asignar y desasignar controlan a qué worktree apunta una instancia de Coast. Consulta [Filesystem](FILESYSTEM.md) para saber cómo funciona el cambio de worktree a nivel de montaje.

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

Después de asignar, `dev-1` está ejecutando la rama `feature/oauth` con todos sus servicios en funcionamiento.

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

Cuando un Coast se asigna a un nuevo worktree, cada servicio necesita saber cómo manejar el cambio de código. Esto se configura por servicio en tu [Coastfile](COASTFILE_TYPES.md) bajo `[assign]`:

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
- **hot** — intercambiar solo el sistema de archivos. El servicio sigue ejecutándose y recoge los cambios mediante la propagación del montaje y los observadores de archivos (p. ej., un servidor de desarrollo con recarga en caliente).
- **restart** — reiniciar el contenedor del servicio. Úsalo para servicios interpretados que solo necesitan un reinicio del proceso. Este es el valor predeterminado.
- **rebuild** — reconstruir la imagen del servicio y reiniciar. Úsalo cuando el cambio de rama afecta al `Dockerfile` o a dependencias de tiempo de compilación.

También puedes especificar disparadores de reconstrucción para que un servicio solo se reconstruya cuando cambien archivos específicos:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

Si ninguno de los archivos disparadores cambió entre ramas, el servicio omite la reconstrucción incluso si la estrategia está configurada como `rebuild`.

## Worktrees eliminados

Si se elimina un worktree asignado, el demonio `coastd` desasigna automáticamente esa instancia de vuelta a la raíz del repositorio principal de Git.

---

> **Consejo: Reducir la latencia de asignación en bases de código grandes**
>
> Internamente, Coast ejecuta `git ls-files` cada vez que se monta o desmonta un worktree. En bases de código grandes o repositorios con muchos archivos, esto puede añadir una latencia apreciable a las operaciones de asignar y desasignar.
>
> Si hay partes de tu base de código que no necesitan reconstruirse entre asignaciones, puedes indicarle a Coast que las omita usando `exclude_paths` en tu Coastfile:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> Las rutas listadas en `exclude_paths` se ignoran durante la comparación de archivos, lo que puede acelerar significativamente los tiempos de asignación.

# Sistema de archivos

Tu máquina anfitriona y cada instancia de Coast comparten los mismos archivos del proyecto. La raíz del proyecto en el host se monta con permisos de lectura-escritura dentro del contenedor DinD en `/host-project`, y Coast hace un bind-mount del árbol de trabajo activo en `/workspace`. Esto es lo que hace posible que un agente ejecutándose en tu máquina anfitriona edite código mientras los servicios dentro de Coast recogen los cambios en tiempo real.

## El montaje compartido

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

La raíz del proyecto en el host se monta con permisos de lectura-escritura en `/host-project` dentro del [contenedor DinD](RUNTIMES_AND_SERVICES.md) cuando se crea el contenedor. Después de que el contenedor se inicia, un `mount --bind /host-project /workspace` dentro del contenedor crea la ruta de trabajo `/workspace` con propagación de montaje compartida (`mount --make-rshared`), de modo que los servicios internos de compose que hacen bind-mount de subdirectorios de `/workspace` vean el contenido correcto.

Este enfoque en dos etapas existe por una razón: el bind mount de Docker en `/host-project` queda fijo en la creación del contenedor y no puede cambiarse sin recrear el contenedor. Pero el bind mount de Linux en `/workspace` dentro del contenedor puede desmontarse y volver a enlazarse a un subdirectorio diferente —un worktree— sin tocar el ciclo de vida del contenedor. Esto es lo que hace que `coast assign` sea rápido.

`/workspace` es de lectura-escritura. Los cambios de archivos fluyen en ambas direcciones al instante. Guarda un archivo en el host y un servidor de desarrollo dentro de Coast lo detecta. Crea un archivo dentro de Coast y aparece en el host.

## Agentes en el host y Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

Debido a que el sistema de archivos se comparte, un agente de codificación con IA que se ejecuta en el host puede editar archivos libremente y los servicios en ejecución dentro de Coast ven los cambios de inmediato. El agente no necesita ejecutarse dentro del contenedor de Coast —opera desde el host con normalidad.

Cuando el agente necesita información de ejecución —logs, estado de servicios, salida de pruebas— llama a comandos del CLI de Coast desde el host:

- `coast logs dev-1 --service web --tail 50` para la salida del servicio (ver [Logs](LOGS.md))
- `coast ps dev-1` para el estado del servicio (ver [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` para ejecutar comandos dentro de Coast (ver [Exec & Docker](EXEC_AND_DOCKER.md))

Esta es la ventaja arquitectónica fundamental: **la edición de código ocurre en el host, la ejecución ocurre en Coast, y el sistema de archivos compartido los conecta.** El agente en el host nunca necesita estar "dentro" de Coast para hacer su trabajo.

## Cambio de worktree

Cuando `coast assign` cambia un Coast a un worktree diferente, vuelve a montar `/workspace` para que apunte a ese worktree de git en lugar de la raíz del proyecto:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

El worktree se crea en el host en `{project_root}/.worktrees/{worktree_name}`. El nombre del directorio `.worktrees` es configurable mediante `worktree_dir` en tu Coastfile y debería estar en tu `.gitignore`.

Si el worktree es nuevo, Coast inicializa ciertos archivos ignorados por git desde la raíz del proyecto antes del remount. Enumera los archivos ignorados con `git ls-files --others --ignored --exclude-standard`, filtra directorios pesados comunes más cualquier `exclude_paths` configurado, y luego usa `rsync --files-from` con `--link-dest` para enlazar por hardlink los archivos seleccionados dentro del worktree. Coast registra esa inicialización en metadatos internos del worktree y la omite en asignaciones posteriores al mismo worktree a menos que la refresques explícitamente con `coast assign --force-sync`.

Dentro del contenedor, `/workspace` se desmonta de forma perezosa (lazy-unmounted) y se vuelve a enlazar al subdirectorio del worktree en `/host-project/.worktrees/{branch_name}`. Este remount es rápido —no recrea el contenedor DinD ni reinicia el daemon interno de Docker. Los servicios de compose y los servicios bare aún pueden recrearse o reiniciarse después del remount para que sus bind mounts se resuelvan a través del nuevo `/workspace`.

Los directorios grandes de dependencias como `node_modules` no forman parte de esta ruta genérica de bootstrap. Esos normalmente se gestionan mediante cachés o volúmenes específicos del servicio.

Si usas `[assign.rebuild_triggers]`, Coast también ejecuta `git diff --name-only <previous>..<worktree>` en el host para decidir si un servicio marcado como `rebuild` puede degradarse a `restart`. Consulta [Assign and Unassign](ASSIGN.md) y [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) para los detalles que afectan la latencia de assign.

`coast unassign` revierte `/workspace` a `/host-project` (la raíz del proyecto). `coast start` después de un stop vuelve a aplicar el montaje correcto según si la instancia tiene un worktree asignado.

## Todos los montajes

Cada contenedor de Coast tiene estos montajes:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Raíz del proyecto o worktree. Conmutable al asignar. |
| `/host-project` | Docker bind mount | RW | Raíz del proyecto sin procesar. Fijo en la creación del contenedor. |
| `/image-cache` | Docker bind mount | RO | Tarballs OCI predescargados desde `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Artefacto de build con archivos de compose reescritos. |
| `/coast-override` | Docker bind mount | RO | Overrides de compose generados para [servicios compartidos](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Estado del daemon interno de Docker. Persiste a través de la eliminación del contenedor. |

Los montajes de solo lectura son infraestructura: transportan el artefacto de build, las imágenes en caché y los overrides de compose que Coast genera. Interactúas con ellos indirectamente mediante `coast build` y el Coastfile. Los montajes de lectura-escritura son donde vive tu código y donde el daemon interno almacena su estado.

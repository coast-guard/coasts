# Sistema de archivos

Tu máquina anfitriona y cada instancia de Coast comparten los mismos archivos del proyecto. La raíz del proyecto en el host se monta mediante bind dentro del contenedor DinD en `/workspace`, por lo que las ediciones en el host aparecen instantáneamente dentro de Coast y viceversa. Esto es lo que hace posible que un agente que se ejecuta en tu máquina anfitriona edite código mientras los servicios dentro de Coast detectan los cambios en tiempo real.

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

La raíz del proyecto en el host se monta en modo lectura-escritura en `/host-project` dentro del [contenedor DinD](RUNTIMES_AND_SERVICES.md) cuando se crea el contenedor. Después de que el contenedor arranca, un `mount --bind /host-project /workspace` dentro del contenedor crea la ruta de trabajo `/workspace` con propagación de montaje compartida (`mount --make-rshared`), de modo que los servicios internos de compose que montan subdirectorios de `/workspace` mediante bind vean el contenido correcto.

Este enfoque en dos etapas existe por una razón: el montaje bind de Docker en `/host-project` queda fijo al crearse el contenedor y no se puede cambiar sin recrearlo. Pero el montaje bind de Linux en `/workspace` dentro del contenedor puede desmontarse y volver a montarse apuntando a un subdirectorio diferente —un worktree— sin tocar el ciclo de vida del contenedor. Esto es lo que hace que `coast assign` sea rápido.

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

Como el sistema de archivos está compartido, un agente de codificación con IA que se ejecuta en el host puede editar archivos libremente y los servicios en ejecución dentro de Coast ven los cambios de inmediato. El agente no necesita ejecutarse dentro del contenedor de Coast: opera desde el host con normalidad.

Cuando el agente necesita información de runtime —logs, estado de servicios, salida de pruebas— invoca comandos de la CLI de Coast desde el host:

- `coast logs dev-1 --service web --tail 50` para la salida del servicio (ver [Logs](LOGS.md))
- `coast ps dev-1` para el estado del servicio (ver [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` para ejecutar comandos dentro de Coast (ver [Exec & Docker](EXEC_AND_DOCKER.md))

Esta es la ventaja arquitectónica fundamental: **la edición de código ocurre en el host, el runtime ocurre en Coast y el sistema de archivos compartido los conecta.** El agente del host nunca necesita estar "dentro" de Coast para hacer su trabajo.

## Cambio de worktree

Cuando `coast assign` cambia una instancia de Coast a un worktree diferente, vuelve a montar `/workspace` para que apunte a ese worktree de git en lugar de a la raíz del proyecto:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

El worktree se crea en el host en `{project_root}/.worktrees/{worktree_name}`. El nombre del directorio `.worktrees` es configurable mediante `worktree_dir` en tu Coastfile y debería estar en tu `.gitignore`.

Dentro del contenedor, `/workspace` se desmonta (lazy-unmount) y se vuelve a montar apuntando al subdirectorio del worktree en `/host-project/.worktrees/{branch_name}`. Este remontaje es rápido: no recrea el contenedor DinD ni reinicia el daemon interno de Docker. Los servicios internos de compose se recrean para que sus montajes bind se resuelvan a través del nuevo `/workspace`.

Los archivos ignorados por git como `node_modules` se sincronizan desde la raíz del proyecto hacia el worktree mediante rsync con hardlinks, de modo que la configuración inicial es casi instantánea incluso para árboles de dependencias grandes.

En macOS, la E/S de archivos entre el host y la VM de Docker tiene una sobrecarga inherente. Coast ejecuta `git ls-files` durante assign y unassign para hacer diff del worktree, y en bases de código grandes esto puede añadir una latencia perceptible. Si partes de tu proyecto no necesitan ser diffadas entre assigns (docs, fixtures de prueba, scripts), puedes excluirlas con `exclude_paths` en tu Coastfile para reducir esta sobrecarga. Consulta [Assign and Unassign](ASSIGN.md) para más detalles.

`coast unassign` revierte `/workspace` de nuevo a `/host-project` (la raíz del proyecto). `coast start` después de un stop vuelve a aplicar el montaje correcto según si la instancia tiene un worktree asignado.

## Todos los montajes

Cada contenedor de Coast tiene estos montajes:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Raíz del proyecto o worktree. Conmutable en assign. |
| `/host-project` | Docker bind mount | RW | Raíz del proyecto sin procesar. Fija al crearse el contenedor. |
| `/image-cache` | Docker bind mount | RO | Tarballs OCI precargadas desde `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Artefacto de build con archivos de compose reescritos. |
| `/coast-override` | Docker bind mount | RO | Overrides de compose generadas para [servicios compartidos](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Estado del daemon interno de Docker. Persiste a través de la eliminación del contenedor. |

Los montajes de solo lectura son infraestructura: transportan el artefacto de build, las imágenes en caché y las overrides de compose que Coast genera. Interactúas con ellos de forma indirecta mediante `coast build` y el Coastfile. Los montajes de lectura-escritura son donde vive tu código y donde el daemon interno almacena su estado.

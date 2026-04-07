# Sincronización de archivos

Los coasts remotos usan una estrategia de sincronización de dos capas: rsync para transferencias masivas, mutagen para sincronización continua en tiempo real. Ambas herramientas son dependencias de tiempo de ejecución instaladas dentro de los contenedores de coast; no se requieren en su máquina anfitriona.

## Dónde se ejecuta la sincronización

```text
Local Machine                          Remote Machine
┌─────────────────────────────┐        ┌──────────────────────────────┐
│  coastd daemon              │        │                              │
│    │                        │        │                              │
│    │ rsync (direct SSH)     │  SSH   │  /data/workspaces/{p}/{i}/   │
│    │────────────────────────│───────▶│    (rsync writes here)       │
│    │                        │        │    │                         │
│    │ docker exec            │        │    │ bind mount              │
│    ▼                        │        │    ▼                         │
│  Shell Container            │  SSH   │  Remote DinD Container       │
│    /workspace (bind mount)  │───────▶│    /workspace                │
│    mutagen (continuous sync)│        │    (compose services running)│
│    SSH key (copied in)      │        │                              │
└─────────────────────────────┘        └──────────────────────────────┘
```

El daemon ejecuta rsync directamente desde el proceso del host. Mutagen se ejecuta dentro del contenedor shell local mediante `docker exec`.

## Capa 1: rsync (transferencia masiva)

En `coast run` y `coast assign`, el daemon ejecuta rsync desde el host para transferir archivos del espacio de trabajo al remoto:

```bash
rsync -rlDzP --delete-after \
  --rsync-path="sudo rsync" \
  --exclude '.git' --exclude 'node_modules' \
  --exclude 'target' --exclude '__pycache__' \
  --exclude '.react-router' --exclude '.next' \
  -e "ssh -p {port} -i {key}" \
  {local_workspace}/ {user}@{host}:{remote_workspace}/
```

Después de que rsync se complete, el daemon ejecuta `sudo chown -R` en el remoto para dar al usuario SSH la propiedad de los archivos. rsync se ejecuta como root mediante `--rsync-path="sudo rsync"` porque el espacio de trabajo remoto puede contener archivos propiedad de root provenientes de operaciones de coast-service dentro del contenedor.

### Lo que rsync hace bien

- **Transferencias iniciales.** El primer `coast run` envía todo el espacio de trabajo.
- **Cambios de worktree.** `coast assign` envía solo el delta entre el worktree anterior y el nuevo. Los archivos que no cambiaron no se retransmiten.
- **Compresión.** La bandera `-z` comprime los datos en tránsito.

### Rutas excluidas

rsync omite rutas que no deben transferirse:

| Path | Why |
|------|-----|
| `.git` | Grande, no se necesita en el remoto (el contenido del worktree es suficiente) |
| `node_modules` | Reconstruido dentro de DinD a partir de lockfiles |
| `target` | Artefactos de compilación de Rust/Go, reconstruidos en el remoto |
| `__pycache__` | Caché de bytecode de Python, regenerada |
| `.react-router` | Tipos generados, recreados por el servidor de desarrollo |
| `.next` | Caché de compilación de Next.js, regenerada |

### Protección de archivos generados

Cuando `coast assign` se ejecuta con `--delete-after`, rsync normalmente elimina en el remoto los archivos que no existen localmente. Esto destruiría archivos generados (como clientes proto en `generated/`) que el servidor de desarrollo remoto creó pero que su worktree local no contiene.

Para evitar esto, rsync usa reglas `--filter 'P generated/***'` que protegen directorios generados específicos de la eliminación. Las rutas protegidas incluyen `generated/`, `.react-router/`, `internal/generated/` y `app/generated/`.

### Manejo de transferencias parciales

El código de salida 23 de rsync (transferencia parcial) se trata como una advertencia no fatal. Esto maneja una condición de carrera en la que los servidores de desarrollo en ejecución dentro del DinD remoto regeneran archivos (por ejemplo, `.react-router/types/`) mientras rsync está escribiendo. Los archivos fuente se transfieren correctamente; solo pueden fallar los artefactos generados, y esos se regeneran de todos modos por el servidor de desarrollo.

## Capa 2: mutagen (sincronización continua)

Después del rsync inicial, el daemon inicia una sesión de mutagen dentro del contenedor shell local:

```bash
docker exec {shell_container} mutagen sync create \
    --name coast-{project}-{instance} \
    --sync-mode one-way-safe \
    --ignore-vcs \
    --ignore node_modules --ignore target \
    --ignore __pycache__ --ignore .next \
    /workspace/ {user}@{host}:{remote_workspace}/
```

Mutagen observa cambios de archivos mediante eventos a nivel del sistema operativo (inotify dentro del contenedor), agrupa los cambios y transfiere deltas a través de una conexión SSH persistente. Sus ediciones aparecen en el remoto en cuestión de segundos.

### Modo one-way-safe

Mutagen se ejecuta en modo `one-way-safe`: los cambios fluyen solo de local a remoto. Los archivos creados en el remoto (por servidores de desarrollo, herramientas de compilación, etc.) no se sincronizan de vuelta a su máquina local. Esto evita que los artefactos generados contaminen su directorio de trabajo.

### Mutagen es una dependencia de tiempo de ejecución

Mutagen está instalado en:

- La **imagen de coast** (construida por `coast build` a partir de `[coast.setup]`), usada por el contenedor shell local.
- La **imagen Docker de coast-service** (`Dockerfile.coast-service`), usada en el lado remoto.

El daemon nunca ejecuta mutagen directamente en el host. Orquesta mediante `docker exec` dentro del contenedor shell.

## Ciclo de vida

| Command | rsync | mutagen |
|---------|-------|---------|
| `coast run` | Transferencia completa inicial | Sesión creada después de rsync |
| `coast assign` | Transferencia delta del nuevo worktree | Sesión anterior terminada, nueva sesión creada |
| `coast stop` | -- | Sesión terminada |
| `coast rm` | -- | Sesión terminada |

### Comportamiento de respaldo

Si la sesión de mutagen no logra iniciarse dentro del contenedor shell, el daemon registra una advertencia. El rsync inicial todavía proporciona el contenido del espacio de trabajo, pero los cambios de archivos no se sincronizarán en tiempo real hasta que la sesión se restablezca (por ejemplo, en el siguiente `coast assign` o reinicio del daemon).

## Configuración de la estrategia de sincronización

La sección `[remote]` de su Coastfile controla la estrategia de sincronización:

```toml
[remote]
workspace_sync = "mutagen"    # "rsync" (default) or "mutagen"
```

- **`rsync`** (predeterminado): solo se ejecuta la transferencia inicial con rsync. Sin sincronización continua. Bueno para entornos de CI o trabajos por lotes donde no se necesita sincronización en tiempo real.
- **`mutagen`**: rsync para la transferencia inicial, luego mutagen para sincronización continua. Úselo para desarrollo interactivo donde quiere que las ediciones aparezcan en el remoto de inmediato.

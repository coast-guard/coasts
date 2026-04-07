# CLI y configuración

Esta página cubre el grupo de comandos `coast remote`, el formato de configuración `Coastfile.remote` y la gestión de disco para máquinas remotas.

## Comandos de gestión remota

### `coast remote add`

Registrar una máquina remota con el daemon:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

Ejemplos:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

Los detalles de conexión se almacenan en `state.db` del daemon. Nunca se almacenan en los Coastfiles.

### `coast remote ls`

Listar todos los remotos registrados:

```bash
coast remote ls
```

### `coast remote rm`

Eliminar un remoto registrado:

```bash
coast remote rm <name>
```

Si todavía hay instancias en ejecución en el remoto, elimínalas primero con `coast rm`.

### `coast remote test`

Verificar la conectividad SSH y la disponibilidad de coast-service:

```bash
coast remote test <name>
```

Esto comprueba el acceso SSH, confirma que se puede acceder a coast-service en el puerto 31420 a través del túnel SSH e informa la arquitectura del remoto y la versión de coast-service.

### `coast remote prune`

Limpiar recursos huérfanos en una máquina remota:

```bash
coast remote prune <name>              # remove orphaned resources
coast remote prune <name> --dry-run    # preview what would be removed
```

Prune identifica los recursos huérfanos cruzando volúmenes de Docker y directorios de workspace con la base de datos de instancias de coast-service. Los recursos que pertenecen a instancias activas nunca se eliminan.

## Configuración de Coastfile

Los coasts remotos usan un Coastfile separado que extiende tu configuración base. El nombre del archivo determina el tipo:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### Ejemplo mínimo

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### La sección `[remote]`

La sección `[remote]` declara las preferencias de sincronización. Los detalles de conexión (host, user, SSH key) provienen de `coast remote add` y se resuelven en tiempo de ejecución.

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | Estrategia de sincronización: `"rsync"` solo para una transferencia masiva única, `"mutagen"` para rsync + sincronización continua en tiempo real |

### Restricciones de validación

1. La sección `[remote]` es obligatoria cuando el tipo de Coastfile comienza con `remote`.
2. Los Coastfiles no remotos no pueden tener una sección `[remote]`.
3. La configuración inline de host no es compatible. Los detalles de conexión deben provenir de un remoto registrado.
4. Los volúmenes compartidos con `strategy = "shared"` crean un volumen de Docker en el host remoto, compartido entre todos los coasts de ese remoto. El volumen no se distribuye entre diferentes máquinas remotas.

### Herencia

Los Coastfiles remotos usan el mismo [sistema de herencia](../coastfiles/INHERITANCE.md) que otros Coastfiles tipados. La directiva `extends = "Coastfile"` fusiona la configuración base con las anulaciones remotas. Puedes sobrescribir puertos, servicios, volúmenes y asignar estrategias igual que con cualquier otra variante tipada.

## Gestión de disco

### Uso de recursos por instancia

Cada instancia de coast remota consume aproximadamente:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

Disco mínimo recomendado: **50 GB** para proyectos típicos con 2-3 instancias concurrentes.

### Convenciones de nombres de recursos

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### Limpieza en `coast rm`

Cuando `coast rm` elimina una instancia remota, limpia:

1. El contenedor remoto de DinD (a través de coast-service)
2. El volumen Docker de DinD (`coast-dind--{project}--{name}`)
3. El directorio de workspace (`/data/workspaces/{project}/{name}`)
4. El registro local de la instancia shadow, las asignaciones de puertos y el contenedor shell

### Cuándo ejecutar prune

Si `df -h` en el remoto muestra un uso de disco alto después de eliminar instancias, pueden haber quedado recursos huérfanos de operaciones fallidas o interrumpidas. Ejecuta `coast remote prune` para recuperar espacio:

```bash
# See what would be removed
coast remote prune my-vm --dry-run

# Actually remove
coast remote prune my-vm
```

Prune nunca elimina recursos que pertenecen a instancias activas.

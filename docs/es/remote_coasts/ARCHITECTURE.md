# Arquitectura

Un coast remoto divide la ejecución entre tu máquina local y un servidor remoto. La experiencia de desarrollo no cambia porque el daemon enruta transparentemente cada operación a través de un túnel SSH.

## La división en dos contenedores

Cada coast remoto crea dos contenedores:

### Shell Coast (local)

Un contenedor Docker liviano en tu máquina. Tiene los mismos bind mounts que un coast normal (`/host-project`, `/workspace`) pero no tiene un daemon Docker interno ni servicios de compose. Su entrypoint es `sleep infinity`.

El shell coast existe por una razón: preserva el [puente del sistema de archivos](../concepts_and_terminology/FILESYSTEM.md) para que los agentes y editores del lado del host puedan editar archivos bajo `/workspace`. Esas ediciones se sincronizan con el remoto mediante [rsync y mutagen](FILE_SYNC.md).

### Remote Coast (remoto)

Gestionado por `coast-service` en la máquina remota. Aquí es donde ocurre el trabajo real: un contenedor DinD completo que ejecuta tus servicios de compose, con puertos dinámicos asignados para cada servicio.

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                            │
│                                                                          │
│  ┌────────────┐    unix     ┌───────────────────────────────────────┐    │
│  │ coast CLI  │───socket───▶│ coast-daemon                         │    │
│  └────────────┘             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shell Coast (sleep infinity)    │  │    │
│                             │  │ - /host-project (bind mount)    │  │    │
│                             │  │ - /workspace (mount --bind)     │  │    │
│                             │  │ - NO inner docker               │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Port Manager                    │  │    │
│                             │  │ - allocates local dynamic ports │  │    │
│                             │  │ - SSH -L tunnels to remote      │  │    │
│                             │  │   dynamic ports                 │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shared Services (local)         │  │    │
│                             │  │ - postgres, redis, etc.         │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  state.db (shadow instance,           │    │
│                             │           remote_host, port allocs)   │    │
│                             └───────────────────┬───────────────────┘    │
│                                                 │                        │
│                                    SSH tunnel   │  rsync / SSH           │
│                                                 │                        │
└─────────────────────────────────────────────────┼────────────────────────┘
                                                  │
┌─────────────────────────────────────────────────┼────────────────────────┐
│ REMOTE MACHINE                                  │                        │
│                                                 ▼                        │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │ coast-service (HTTP API on :31420)                                │   │
│  │                                                                   │   │
│  │  ┌───────────────────────────────────────────────────────────┐    │   │
│  │  │ DinD Container (per instance)                             │    │   │
│  │  │  /workspace (synced from local)                           │    │   │
│  │  │  compose services / bare services                         │    │   │
│  │  │  published on dynamic ports (e.g. :52340 -> :3000)        │    │   │
│  │  └───────────────────────────────────────────────────────────┘    │   │
│  │                                                                   │   │
│  │  Port Manager (dynamic port allocation per instance)              │   │
│  │  Build artifacts (/data/images/)                                  │   │
│  │  Image cache (/data/image-cache/)                                 │   │
│  │  Keystore (encrypted secrets)                                     │   │
│  │  remote-state.db (instances, worktrees)                           │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## Capa de túnel SSH

El daemon conecta lo local y lo remoto usando dos tipos de túneles SSH:

### Túneles forward (local a remoto)

Para cada puerto de servicio, el daemon crea un túnel `ssh -L` que mapea un puerto dinámico local al puerto dinámico remoto correspondiente. Esto es lo que hace que `localhost:{dynamic_port}` alcance el servicio remoto.

```text
ssh -N -L {local_dynamic}:localhost:{remote_dynamic} user@remote
```

Cuando ejecutas `coast ports`, la columna dynamic muestra estos endpoints de túneles locales.

### Túneles reverse (remoto a local)

Los [servicios compartidos](../concepts_and_terminology/SHARED_SERVICES.md) (Postgres, Redis, etc.) se ejecutan en tu máquina local. El daemon crea túneles `ssh -R` para que el contenedor DinD remoto pueda alcanzarlos:

```text
ssh -N -R 0.0.0.0:{remote_port}:localhost:{local_port} user@remote
```

Dentro del contenedor DinD remoto, los servicios se conectan a los servicios compartidos mediante `host.docker.internal:{port}`, que se resuelve al gateway del bridge de Docker donde el túnel reverse está escuchando.

El sshd del host remoto debe tener `GatewayPorts clientspecified` habilitado para que los túneles reverse puedan enlazarse en `0.0.0.0` en lugar de `127.0.0.1`.

### Recuperación de túneles

Los túneles SSH pueden romperse cuando tu laptop entra en suspensión o cambia la red. El daemon ejecuta un bucle de salud en segundo plano que:

1. Sondea cada puerto dinámico cada 5 segundos mediante una conexión TCP.
2. Si todos los puertos de una instancia están caídos, mata los procesos de túnel obsoletos de esa instancia y los restablece.
3. Si solo algunos puertos están caídos (fallo parcial), restablece solo los túneles faltantes sin interrumpir los que están sanos.
4. Limpia enlaces de puertos remotos obsoletos mediante `fuser -k` antes de crear nuevos túneles reverse.

La recuperación es por instancia -- recuperar los túneles de una instancia nunca interrumpe los de otra.

## Cadena de reenvío de puertos

Todos los puertos son dinámicos en la capa intermedia. Los puertos canónicos solo existen en los endpoints: dentro del contenedor DinD donde los servicios escuchan, y en tu localhost mediante [`coast checkout`](../concepts_and_terminology/CHECKOUT.md).

```text
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

Esta cadena de tres saltos permite múltiples instancias del mismo proyecto en una sola máquina remota sin conflictos de puertos. Cada instancia obtiene su propio conjunto de puertos dinámicos en ambos lados.

## Enrutamiento de solicitudes

Cada handler del daemon verifica `remote_host` en la instancia. Si está configurado, la solicitud se reenvía a coast-service a través del túnel SSH:

| Command | Comportamiento remoto |
|---------|------------------------|
| `coast run` | Crear shell coast localmente + transferir artefactos + reenviar a coast-service |
| `coast build` | Compilar en la máquina remota (sin reenvío de compilación local) |
| `coast assign` | Hacer rsync del nuevo contenido del worktree + reenviar solicitud de assign |
| `coast exec` | Reenviar a coast-service |
| `coast ps` | Reenviar a coast-service |
| `coast logs` | Reenviar a coast-service |
| `coast stop` | Reenviar + matar túneles SSH locales |
| `coast start` | Reenviar + restablecer túneles SSH |
| `coast rm` | Reenviar + matar túneles + eliminar instancia shadow local |
| `coast checkout` | Solo local (socat en el host, no se necesita reenvío) |
| `coast secret set` | Almacenar localmente + reenviar al keystore remoto |

## coast-service

`coast-service` es el plano de control que se ejecuta en la máquina remota. Es un servidor HTTP (Axum) que escucha en el puerto 31420 y refleja las operaciones locales del daemon: build, run, assign, exec, ps, logs, stop, start, rm, secrets y reinicios de servicios.

Gestiona su propia base de datos de estado SQLite, contenedores Docker (DinD), asignación de puertos dinámicos, artefactos de compilación, caché de imágenes y keystore cifrado. El daemon se comunica con él exclusivamente a través del túnel SSH -- coast-service nunca se expone al internet público.

Consulta [Setup](SETUP.md) para ver las instrucciones de despliegue.

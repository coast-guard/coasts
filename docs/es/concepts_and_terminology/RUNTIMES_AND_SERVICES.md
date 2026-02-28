# Runtimes y servicios

Un Coast se ejecuta dentro de un runtime de contenedores: un contenedor externo que aloja su propio daemon de Docker (o Podman). Los servicios de tu proyecto se ejecutan dentro de ese daemon interno, completamente aislados de otras instancias de Coast. Actualmente, **DinD (Docker-in-Docker) es el único runtime probado en producción.** En este momento te recomendamos que te quedes con DinD hasta que el soporte para Podman y Sysbox haya sido probado a fondo.

## Runtimes

El campo `runtime` en tu Coastfile selecciona qué runtime de contenedores respalda a Coast. El valor predeterminado es `dind` y puedes omitirlo por completo:

```toml
[coast]
name = "my-app"
runtime = "dind"
```

Se aceptan tres valores: `dind`, `sysbox` y `podman`. En la práctica, solo DinD está conectado al daemon y ha sido probado de extremo a extremo.

### DinD (Docker-in-Docker)

El runtime predeterminado y el único que deberías usar hoy. Coast crea un contenedor a partir de la imagen `docker:dind` con el modo `--privileged` habilitado. Dentro de ese contenedor, se inicia un daemon de Docker completo y tus servicios de `docker-compose.yml` se ejecutan como contenedores anidados.

DinD está completamente integrado:

- Las imágenes se precargan en el host y se cargan en el daemon interno en `coast run`
- Las imágenes por instancia se construyen en el host y se canalizan mediante `docker save | docker load`
- El estado del daemon interno se persiste en un volumen con nombre (`coast-dind--{project}--{instance}`) en `/var/lib/docker`, de modo que las ejecuciones posteriores omiten por completo la carga de imágenes
- Los puertos se publican directamente desde el contenedor DinD hacia el host
- Las anulaciones de Compose, el puenteo de red compartida de servicios, la inyección de secretos y las estrategias de volúmenes funcionan

### Sysbox (futuro)

Sysbox es un runtime OCI solo para Linux que proporciona contenedores sin privilegios (rootless) sin `--privileged`. Usaría `--runtime=sysbox-runc` en lugar del modo privilegiado, lo cual ofrece una mejor postura de seguridad. La implementación del trait existe en la base de código pero no está conectada al daemon. No funciona en macOS.

### Podman (futuro)

Podman reemplazaría el daemon interno de Docker por un daemon de Podman que se ejecuta dentro de `quay.io/podman/stable`, usando `podman-compose` en lugar de `docker compose`. La implementación del trait existe pero no está conectada al daemon.

Cuando el soporte para Sysbox y Podman se estabilice, esta página se actualizará. Por ahora, deja `runtime` como `dind` u omítelo.

## Arquitectura de Docker-in-Docker

Cada Coast es un contenedor anidado. El daemon de Docker del host gestiona el contenedor DinD externo, y el daemon de Docker interno dentro de este gestiona tus servicios de compose.

```text
Máquina host
│
├── Docker daemon (host)
│   │
│   ├── contenedor coast: dev-1 (docker:dind, --privileged)
│   │   │
│   │   ├── Docker daemon interno
│   │   │   ├── web        (tu app, :3000)
│   │   │   ├── postgres   (base de datos, :5432)
│   │   │   └── redis      (caché, :6379)
│   │   │
│   │   ├── /workspace          ← bind mount de la raíz de tu proyecto
│   │   ├── /image-cache        ← montaje de solo lectura de ~/.coast/image-cache/
│   │   ├── /coast-artifact     ← montaje de solo lectura del artefacto de build
│   │   ├── /coast-override     ← anulaciones de compose generadas
│   │   └── /var/lib/docker     ← volumen con nombre (estado del daemon interno)
│   │
│   ├── contenedor coast: dev-2 (docker:dind, --privileged)
│   │   └── (misma estructura, totalmente aislado)
│   │
│   └── postgres compartido (nivel host, red bridge)
│
└── ~/.coast/
    ├── image-cache/    ← tarballs OCI compartidos entre todos los proyectos
    └── state.db        ← metadatos de instancias
```

Cuando `coast run` crea una instancia, realiza lo siguiente:

1. Crea e inicia el contenedor DinD en el daemon del host
2. Sondea `docker info` dentro del contenedor hasta que el daemon interno esté listo (hasta 120 segundos)
3. Comprueba qué imágenes ya tiene el daemon interno (desde el volumen persistente `/var/lib/docker`) y carga cualquier tarball faltante desde la caché
4. Canaliza las imágenes por instancia construidas en el host mediante `docker save | docker load`
5. Vincula `/host-project` a `/workspace` para que los servicios de compose vean tu código fuente
6. Ejecuta `docker compose up -d` dentro del contenedor y espera a que todos los servicios estén en ejecución o saludables

El volumen persistente `/var/lib/docker` es la optimización clave. En un `coast run` nuevo, cargar imágenes en el daemon interno puede tardar más de 20 segundos. En ejecuciones posteriores (incluso después de `coast rm` y volver a ejecutar), el daemon interno ya tiene las imágenes en caché y el arranque baja a menos de 10 segundos.

## Servicios

Los servicios son los contenedores (o procesos, en el caso de los [servicios bare](BARE_SERVICES.md)) que se ejecutan dentro de tu Coast. Para un Coast basado en compose, estos son los servicios definidos en tu `docker-compose.yml`.

![Pestaña Servicios en Coastguard](../../assets/coastguard-services.png)
*La pestaña Servicios de Coastguard mostrando servicios de compose, su estado, imágenes y asignaciones de puertos.*

La pestaña Servicios en Coastguard muestra cada servicio que se ejecuta dentro de una instancia de Coast:

- **Service** — el nombre del servicio de compose (p. ej., `web`, `backend`, `redis`). Haz clic para ver datos detallados de inspección, logs y estadísticas de ese contenedor.
- **Status** — si el servicio está en ejecución, detenido o en un estado de error.
- **Image** — la imagen de Docker desde la que se construye el servicio.
- **Ports** — las asignaciones de puertos sin procesar de compose y los [puertos canónicos/dinámicos](PORTS.md) gestionados por coast. Los puertos dinámicos siempre son accesibles; los puertos canónicos solo enrutan a la instancia [checked-out](CHECKOUT.md).

Puedes seleccionar múltiples servicios y detenerlos, iniciarlos, reiniciarlos o eliminarlos en lote desde la barra de herramientas.

Los servicios que están configurados como [servicios compartidos](SHARED_SERVICES.md) se ejecutan en el daemon del host en lugar de dentro de Coast, por lo que no aparecen en esta lista. Tienen su propia pestaña.

## `coast ps`

El equivalente en la CLI de la pestaña Servicios es `coast ps`:

```bash
coast ps dev-1
```

```text
Services in coast instance 'dev-1':
  NAME                      STATUS               PORTS
  backend                   running              0.0.0.0:8080->8080/tcp, 0.0.0.0:40000->40000/tcp
  mailhog                   running              0.0.0.0:1025->1025/tcp, 0.0.0.0:8025->8025/tcp
  reach-web                 running              0.0.0.0:4000->4000/tcp
  test-redis                running              0.0.0.0:6380->6379/tcp
  web                       running              0.0.0.0:3000->3000/tcp
```

Bajo el capó, el daemon ejecuta `docker compose ps --format json` dentro del contenedor DinD y analiza la salida JSON. Los resultados pasan por varios filtros antes de devolverse:

- Se eliminan los **servicios compartidos** — se ejecutan en el host, no dentro de Coast.
- Los **trabajos de una sola ejecución** (servicios sin puertos) se ocultan una vez que salen correctamente. Si fallan, aparecen para que puedas investigar.
- **Servicios faltantes** — si un servicio de larga duración que debería estar presente no aparece en la salida, se añade con un estado `down` para que sepas que algo está mal.

Para una inspección más profunda, usa `coast logs` para seguir la salida del servicio y [`coast exec`](EXEC_AND_DOCKER.md) para obtener una shell dentro del contenedor Coast. Consulta [Logs](LOGS.md) para obtener todos los detalles sobre el streaming de logs y el compromiso del MCP tradeoff.

```bash
coast logs dev-1 --service web --tail 100
coast exec dev-1
```

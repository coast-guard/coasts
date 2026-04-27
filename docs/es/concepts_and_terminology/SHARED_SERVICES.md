# Servicios compartidos

Los servicios compartidos son contenedores de base de datos e infraestructura (Postgres, Redis, MongoDB, etc.) que se ejecutan en el daemon de Docker de tu host en lugar de dentro de un Coast. Las instancias de Coast se conectan a ellos a través de una red puente, por lo que cada Coast se comunica con el mismo servicio en el mismo volumen del host.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*La pestaña de servicios compartidos de Coastguard mostrando Postgres, Redis y MongoDB administrados por el host.*

## Cómo funcionan

Cuando declaras un servicio compartido en tu Coastfile, Coast lo inicia en el daemon del host y lo elimina de la pila de compose que se ejecuta dentro de cada contenedor de Coast. Luego, los Coasts se configuran para enrutar el tráfico del nombre del servicio de vuelta al contenedor compartido mientras se conserva el puerto del lado del contenedor del servicio dentro del Coast.

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

Debido a que los servicios compartidos reutilizan tus volúmenes existentes del host, cualquier dato que ya tengas por haber ejecutado `docker-compose up` localmente está disponible de inmediato para tus Coasts.

Esta distinción importa cuando usas puertos mapeados:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- En el host, el servicio compartido se publica en `localhost:5433`.
- Dentro de cada Coast, los contenedores de la aplicación aún se conectan a `postgis:5432`.
- Un entero simple como `5432` es una forma abreviada del mapeo de identidad `"5432:5432"`.

## Cuándo usar servicios compartidos

- Tu proyecto tiene integraciones MCP que se conectan a una base de datos local — los servicios compartidos permiten que estas sigan funcionando sin descubrimiento dinámico de puertos. Si publicas el servicio compartido en el mismo puerto del host que ya usan tus herramientas (por ejemplo `ports = [5432]`), esas herramientas seguirán funcionando sin cambios. Si lo publicas en un puerto diferente del host (por ejemplo `"5433:5432"`), las herramientas del host deben usar ese puerto del host mientras que los Coasts continúan usando el puerto del contenedor.
- Quieres instancias de Coast más ligeras, ya que no necesitan ejecutar sus propios contenedores de base de datos.
- No necesitas aislamiento de datos entre instancias de Coast (cada instancia ve los mismos datos).
- Estás ejecutando agentes de programación en el host (consulta [Filesystem](FILESYSTEM.md)) y quieres que accedan al estado de la base de datos sin enrutar a través de [`coast exec`](EXEC_AND_DOCKER.md). Con servicios compartidos, las herramientas de base de datos y MCP existentes del agente funcionan sin cambios.

Consulta la página de [Volume Topology](VOLUMES.md) para ver alternativas cuando sí necesitas aislamiento.

## Advertencia sobre desambiguación de volúmenes

Los nombres de los volúmenes de Docker no siempre son globalmente únicos. Si ejecutas `docker-compose up` desde varios proyectos diferentes, los volúmenes del host que Coast adjunta a los servicios compartidos podrían no ser los que esperas.

Antes de iniciar Coasts con servicios compartidos, asegúrate de que el último `docker-compose up` que ejecutaste haya sido desde el proyecto que pretendes usar con Coasts. Esto garantiza que los volúmenes del host coincidan con lo que espera tu Coastfile.

## Solución de problemas

Si tus servicios compartidos parecen estar apuntando al volumen incorrecto del host:

1. Abre la interfaz de [Coastguard](COASTGUARD.md) (`coast ui`).
2. Navega a la pestaña **Shared Services**.
3. Selecciona los servicios afectados y haz clic en **Remove**.
4. Haz clic en **Refresh Shared Services** para recrearlos a partir de la configuración actual de tu Coastfile.

Esto desmonta y recrea los contenedores de servicios compartidos, volviéndolos a adjuntar a los volúmenes correctos del host.

## Servicios compartidos y Coasts remotos

Al ejecutar [remote coasts](REMOTES.md), los servicios compartidos siguen ejecutándose en tu máquina local. El daemon establece túneles SSH inversos (`ssh -R`) para que los contenedores remotos de DinD puedan alcanzarlos a través de `host.docker.internal`. Esto mantiene tu base de datos local compartida con las instancias remotas. El sshd del host remoto debe tener `GatewayPorts clientspecified` habilitado para que los túneles inversos se enlacen correctamente.

## Véase también: grupos de servicios compartidos

Los servicios compartidos en línea escalan bien dentro de un solo proyecto (las instancias hermanas `dev-1`, `dev-2` ... comparten el único contenedor del lado del host dibujado arriba). La fricción aparece **entre proyectos**: dos proyectos Coast diferentes que declaran `[shared_services.postgres] ports = [5432]` intentan enlazar el puerto 5432 del host, y el segundo falla. [Shared Service Groups](../shared_service_groups/README.md) elevan la infraestructura a un DinD por proyecto (llamado `<project>-ssg`) para que el Postgres de cada proyecto escuche en el `:5432` interno sin enlazar directamente el puerto del host. Dos proyectos pueden tener cada uno un Postgres en el 5432 canónico porque ninguno enlaza el 5432 del host -- los consumidores enrutan a través de puertos virtuales estables.

Cada proyecto obtiene su propio SSG -- dos proyectos diferentes obtienen sus propios `<p1>-ssg` y `<p2>-ssg` y nunca comparten estado. El modelo SSG es el primo estructurado del patrón en línea: la misma forma de Coastfile `[shared_services.<name>]`, pero con extracción de secretos en tiempo de compilación, puertos virtuales estables a través de reconstrucciones del SSG, checkout del lado del host y verbos de ciclo de vida (`coast ssg run` / `start` / `stop` / `rm`).

Cuándo migrar de servicios compartidos en línea a un SSG:

- Ejecutas más de un proyecto Coast en esta máquina y necesitan el mismo puerto canónico (por ejemplo, ambos quieren un Postgres en 5432) -- en línea no puede ejecutarlos simultáneamente; SSG sí puede.
- Quieres que las herramientas del lado del host (`psql`, clientes GUI, MCPs) alcancen el Postgres del proyecto en el `localhost:5432` canónico (`coast ssg checkout`).
- Quieres extraer credenciales del servicio desde un llavero o una variable de entorno en tiempo de compilación (`[secrets.<name>]` en el Coastfile del SSG).
- Quieres un único lugar para declarar imágenes de infraestructura, volúmenes y secretos para el proyecto.

La migración es opcional por servicio. Los bloques existentes `[shared_services.*]` en línea siguen funcionando sin cambios.

# Servicios compartidos

Los servicios compartidos son contenedores de base de datos e infraestructura (Postgres, Redis, MongoDB, etc.) que se ejecutan en tu daemon de Docker del host en lugar de dentro de un Coast. Las instancias de Coast se conectan a ellos a través de una red de puente, por lo que cada Coast habla con el mismo servicio en el mismo volumen del host.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*La pestaña de servicios compartidos de Coastguard mostrando Postgres, Redis y MongoDB administrados por el host.*

## Cómo funcionan

Cuando declaras un servicio compartido en tu Coastfile, Coast lo inicia en el daemon del host y lo elimina del stack de compose que se ejecuta dentro de cada contenedor de Coast. Luego, los Coasts se configuran para enrutar sus conexiones de vuelta al host.

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

## Cuándo usar servicios compartidos

- Tu proyecto tiene integraciones MCP que se conectan a una base de datos local — los servicios compartidos permiten que estas sigan funcionando sin reconfiguración. Un MCP de base de datos en tu host que se conecta a `localhost:5432` sigue funcionando porque el Postgres compartido está en el host en ese mismo puerto. Sin descubrimiento dinámico de puertos, sin reconfiguración de MCP. Consulta [MCP Servers](MCP_SERVERS.md) para más información sobre esto.
- Quieres instancias de Coast más livianas, ya que no necesitan ejecutar sus propios contenedores de base de datos.
- No necesitas aislamiento de datos entre instancias de Coast (cada instancia ve los mismos datos).
- Estás ejecutando agentes de programación en el host (consulta [Filesystem](FILESYSTEM.md)) y quieres que accedan al estado de la base de datos sin enrutar a través de [`coast exec`](EXEC_AND_DOCKER.md). Con servicios compartidos, las herramientas de base de datos y los MCP existentes del agente funcionan sin cambios.

Consulta la página [Volume Topology](VOLUMES.md) para ver alternativas cuando sí necesitas aislamiento.

## Advertencia de desambiguación de volúmenes

Los nombres de volúmenes de Docker no siempre son globalmente únicos. Si ejecutas `docker-compose up` desde múltiples proyectos diferentes, es posible que los volúmenes del host a los que Coast conecta los servicios compartidos no sean los que esperas.

Antes de iniciar Coasts con servicios compartidos, asegúrate de que el último `docker-compose up` que ejecutaste fue desde el proyecto que pretendes usar con Coasts. Esto garantiza que los volúmenes del host coincidan con lo que espera tu Coastfile.

## Solución de problemas

Si tus servicios compartidos parecen estar apuntando al volumen incorrecto del host:

1. Abre la UI de [Coastguard](COASTGUARD.md) (`coast ui`).
2. Navega a la pestaña **Shared Services**.
3. Selecciona los servicios afectados y haz clic en **Remove**.
4. Haz clic en **Refresh Shared Services** para recrearlos a partir de la configuración actual de tu Coastfile.

Esto detiene y recrea los contenedores de servicios compartidos, volviendo a conectarlos a los volúmenes correctos del host.

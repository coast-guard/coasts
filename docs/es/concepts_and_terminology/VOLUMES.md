# Topología de volúmenes

Coast ofrece tres estrategias de volúmenes que controlan cómo los servicios con muchos datos (bases de datos, cachés, etc.) almacenan y comparten sus datos entre instancias de Coast. Elegir la estrategia adecuada depende de cuánta aislación necesitas y cuánto overhead puedes tolerar.

## Servicios compartidos

Los [servicios compartidos](SHARED_SERVICES.md) se ejecutan en el daemon de Docker de tu host, fuera de cualquier contenedor de Coast. Servicios como Postgres, MongoDB y Redis permanecen en la máquina host y las instancias de Coast enrutan sus llamadas de vuelta al host a través de una red puente.

```text
Host machine
  |
  +--> Postgres (host daemon, existing volume)
  +--> Redis (host daemon, existing volume)
  |
  +--> Coast: dev-1  --connects to--> host Postgres, host Redis
  +--> Coast: dev-2  --connects to--> host Postgres, host Redis
```

No hay aislación de datos entre instancias — cada Coast habla con la misma base de datos. A cambio obtienes:

- Instancias de Coast más livianas ya que no ejecutan sus propios contenedores de base de datos.
- Tus volúmenes existentes del host se reutilizan directamente, por lo que cualquier dato que ya tengas está disponible de inmediato.
- Las integraciones de MCP que se conectan a tu base de datos local siguen funcionando listas para usar.

Esto se configura en tu [Coastfile](COASTFILE_TYPES.md) bajo `[shared_services]`.

## Volúmenes compartidos

Los volúmenes compartidos montan un único volumen de Docker que se comparte entre todas las instancias de Coast. Los servicios en sí (Postgres, Redis, etc.) se ejecutan dentro de cada contenedor de Coast, pero todos leen y escriben en el mismo volumen subyacente.

```text
Coast: dev-1  --mounts--> shared volume "my-project-postgres"
Coast: dev-2  --mounts--> shared volume "my-project-postgres"
```

Esto aísla tus datos de Coast de lo que haya en tu máquina host, pero las instancias siguen compartiendo datos entre sí. Esto es útil cuando quieres una separación limpia de tu entorno de desarrollo en el host sin el overhead de volúmenes por instancia.

```toml
[volumes.postgres_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Volúmenes aislados

Los volúmenes aislados le dan a cada instancia de Coast su propio volumen independiente. No se comparte ningún dato entre instancias ni con el host. Cada instancia comienza vacía (o desde una instantánea — ver abajo) y diverge de manera independiente.

```text
Coast: dev-1  --mounts--> volume "dev-1-postgres"
Coast: dev-2  --mounts--> volume "dev-2-postgres"
```

Esta es la mejor opción para proyectos con muchas pruebas de integración y que necesitan una verdadera aislación de volúmenes entre entornos paralelos. La contrapartida es un inicio más lento y builds de Coast más grandes, ya que cada instancia mantiene su propia copia de los datos.

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

## Creación de instantáneas

Tanto las estrategias compartida como aislada comienzan con volúmenes vacíos de forma predeterminada. Si quieres que las instancias comiencen con una copia de un volumen existente del host, establece `snapshot_source` con el nombre del volumen de Docker desde el cual copiar:

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"
```

La instantánea se toma en el [momento de build](BUILDS.md). Después de la creación, el volumen de cada instancia diverge de manera independiente — las mutaciones no se propagan de vuelta a la fuente ni a otras instancias.

Coast aún no admite la creación de instantáneas en tiempo de ejecución (p. ej., crear una instantánea de un volumen desde una instancia en ejecución). Esto está planeado para una versión futura.

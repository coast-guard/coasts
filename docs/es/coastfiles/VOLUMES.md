# Volúmenes

Las secciones `[volumes.*]` controlan cómo se manejan los volúmenes de Docker con nombre en las instancias de Coast. Cada volumen se configura con una estrategia que determina si las instancias comparten datos o tienen su propia copia independiente.

Para una visión más amplia del aislamiento de datos en Coast —incluidos los servicios compartidos como alternativa— consulta [Volumes](../concepts_and_terminology/VOLUMES.md).

## Definir un volumen

Cada volumen es una sección TOML con nombre bajo `[volumes]`. Se requieren tres campos:

- **`strategy`** — `"isolated"` o `"shared"`
- **`service`** — el nombre del servicio de compose que usa este volumen
- **`mount`** — la ruta de montaje del volumen en el contenedor

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
```

## Estrategias

### `isolated`

Cada instancia de Coast obtiene su propio volumen independiente. Los datos no se comparten entre instancias. Los volúmenes se crean con `coast run` y se eliminan con `coast rm`.

```toml
[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"
```

Esta es la opción correcta para la mayoría de los volúmenes de bases de datos: cada instancia empieza desde cero y puede modificar los datos libremente sin afectar a otras instancias.

### `shared`

Todas las instancias de Coast usan un único volumen de Docker. Cualquier dato escrito por una instancia es visible para todas las demás.

```toml
[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

Los volúmenes compartidos nunca se eliminan con `coast rm`. Persisten hasta que los elimines manualmente.

Coast muestra una advertencia en tiempo de build si usas `shared` en un volumen conectado a un servicio tipo base de datos. Compartir un único volumen de base de datos entre múltiples instancias concurrentes puede causar corrupción. Si necesitas bases de datos compartidas, usa [shared services](SHARED_SERVICES.md) en su lugar.

Buenos usos para volúmenes compartidos: cachés de dependencias (módulos de Go, caché de npm, caché de pip), cachés de artefactos de build y otros datos donde las escrituras concurrentes son seguras o poco probables.

## Siembra desde instantáneas (snapshot)

Los volúmenes aislados pueden sembrarse a partir de un volumen de Docker existente en el momento de creación de la instancia usando `snapshot_source`. Los datos del volumen fuente se copian al nuevo volumen aislado, que luego diverge de forma independiente.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "db"
mount = "/var/lib/postgresql/data"
```

`snapshot_source` solo es válido con `strategy = "isolated"`. Configurarlo en un volumen compartido es un error.

Esto es útil cuando quieres que cada instancia de Coast comience con un conjunto de datos realista copiado desde tu base de datos de desarrollo en el host, pero quieres que las instancias sean libres de modificar esos datos sin afectar a la fuente ni entre sí.

## Ejemplos

### Bases de datos aisladas, caché de dependencias compartida

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "cache"
mount = "/data"

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"
```

### Stack completo sembrado desde instantáneas

Cada instancia comienza con una copia de los volúmenes de base de datos existentes de tu host y luego diverge de manera independiente.

```toml
[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

### Ejecutor de pruebas con bases de datos limpias por instancia

```toml
[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
service = "mongodb"
mount = "/data/db"
```

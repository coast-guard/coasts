# Servicios compartidos

Las secciones `[shared_services.*]` definen servicios de infraestructura — bases de datos, cachés, brokers de mensajes — que se ejecutan en el daemon de Docker del host en lugar de dentro de contenedores individuales de Coast. Varias instancias de Coast se conectan al mismo servicio compartido a través de una red bridge.

Para saber cómo funcionan los servicios compartidos en tiempo de ejecución, la gestión del ciclo de vida y la resolución de problemas, consulta [Servicios compartidos](../concepts_and_terminology/SHARED_SERVICES.md).

## Definir un servicio compartido

Cada servicio compartido es una sección TOML con nombre bajo `[shared_services]`. El campo `image` es obligatorio; todo lo demás es opcional.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (obligatorio)

La imagen de Docker que se ejecutará en el daemon del host.

### `ports`

Lista de puertos que el servicio expone. Se usa para el enrutamiento de la red bridge entre el servicio compartido y las instancias de Coast.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

Los valores de los puertos deben ser distintos de cero.

### `volumes`

Cadenas de montaje (bind) de volúmenes de Docker para persistir datos. Estos son volúmenes de Docker a nivel del host, no volúmenes gestionados por Coast.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
```

### `env`

Variables de entorno que se pasan al contenedor del servicio.

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_DB = "mydb" }
```

### `auto_create_db`

Cuando es `true`, Coast crea automáticamente una base de datos por instancia dentro del servicio compartido para cada instancia de Coast. El valor predeterminado es `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
```

### `inject`

Inyecta la información de conexión del servicio compartido en las instancias de Coast como una variable de entorno o un archivo. Usa el mismo formato `env:NAME` o `file:/path` que los [secrets](SECRETS.md).

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
inject = "env:DATABASE_URL"
```

## Ciclo de vida

Los servicios compartidos se inician automáticamente cuando se ejecuta la primera instancia de Coast que los referencia. Siguen ejecutándose a través de `coast stop` y `coast rm` — eliminar una instancia no afecta a los datos del servicio compartido. Solo `coast shared rm` detiene y elimina un servicio compartido.

Las bases de datos por instancia creadas por `auto_create_db` también sobreviven a la eliminación de la instancia. Usa `coast shared-services rm` para eliminar el servicio y sus datos por completo.

## Cuándo usar servicios compartidos vs volúmenes

Usa servicios compartidos cuando varias instancias de Coast necesiten hablar con el mismo servidor de base de datos (p. ej., un Postgres compartido donde cada instancia obtiene su propia base de datos). Usa [estrategias de volúmenes](VOLUMES.md) cuando quieras controlar cómo se comparten o aíslan los datos de un servicio interno de compose.

## Ejemplos

### Postgres, Redis y MongoDB

```toml
[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass", POSTGRES_MULTIPLE_DATABASES = "dev_db,test_db" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis_data:/data"]

[shared_services.mongodb]
image = "mongo:latest"
ports = [27017]
volumes = ["infra_mongodb_data:/data/db"]
env = { MONGO_INITDB_ROOT_USERNAME = "myapp", MONGO_INITDB_ROOT_PASSWORD = "myapp_pass" }
```

### Postgres compartido mínimo

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

### Servicios compartidos con bases de datos creadas automáticamente

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

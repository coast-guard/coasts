# Servicios compartidos

Las secciones `[shared_services.*]` definen servicios de infraestructura — bases de datos, cachés, brokers de mensajes — que consume un proyecto Coast. Hay dos modalidades:

- **En línea** — declara `image`, `ports`, `env`, `volumes` directamente en el Coastfile del consumidor. Coast inicia un contenedor en el host y enruta el tráfico de la app del consumidor hacia él. Es lo mejor para proyectos individuales con una sola instancia consumidora, o para servicios muy ligeros.
- **Desde un Grupo de Servicios Compartidos (`from_group = true`)** — el servicio vive en el [Grupo de Servicios Compartidos](../shared_service_groups/README.md) del proyecto (un contenedor DinD separado declarado en `Coastfile.shared_service_groups`). El Coastfile del consumidor solo se suscribe. Es lo mejor cuando quieres extracción de secretos, checkout del lado del host a puertos canónicos, o ejecutas múltiples proyectos Coast en este host y cada uno necesita el mismo puerto canónico (un SSG mantiene Postgres en el `:5432` interno sin enlazar el 5432 del host, por lo que dos proyectos pueden coexistir).

Las dos mitades de esta página documentan cada modalidad por separado.

Para saber cómo funcionan los servicios compartidos en tiempo de ejecución, la gestión del ciclo de vida y la resolución de problemas, consulta [Servicios compartidos (concepto)](../concepts_and_terminology/SHARED_SERVICES.md).

---

## Servicios compartidos en línea

Cada servicio en línea es una sección TOML con nombre bajo `[shared_services]`. El campo `image` es obligatorio; todo lo demás es opcional.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }
```

### `image` (obligatorio)

La imagen de Docker que se ejecutará en el daemon del host.

### `ports`

Lista de puertos que expone el servicio. Coast acepta tanto puertos de contenedor simples como asignaciones de estilo Docker Compose `"HOST:CONTAINER"`.

```toml
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
```

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- Un entero simple como `6379` es una abreviatura de `"6379:6379"`.
- Una cadena mapeada como `"5433:5432"` publica el servicio compartido en el puerto del host
  `5433` mientras sigue siendo accesible dentro de Coast en `service-name:5432`.
- Tanto el puerto del host como el del contenedor deben ser distintos de cero.

### `volumes`

Cadenas bind de volúmenes Docker para persistir datos. Estos son volúmenes Docker a nivel de host, no volúmenes gestionados por Coast.

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

### Ciclo de vida

Los servicios compartidos en línea se inician automáticamente cuando se ejecuta la primera instancia de Coast que los referencia. Siguen ejecutándose a través de `coast stop` y `coast rm` — eliminar una instancia no afecta a los datos del servicio compartido. Solo `coast shared rm` detiene y elimina el servicio.

Las bases de datos por instancia creadas por `auto_create_db` también sobreviven a la eliminación de la instancia. Usa `coast shared-services rm` para eliminar el servicio y sus datos por completo.

### Ejemplos en línea

#### Postgres, Redis y MongoDB

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

#### Postgres compartido mínimo

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Postgres con asignación host/contenedor

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = ["5433:5432"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "coast_demo" }
```

#### Bases de datos creadas automáticamente

```toml
[shared_services.db]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

---

## Servicios compartidos desde un Grupo de Servicios Compartidos

Para proyectos que quieren una configuración estructurada de infraestructura compartida — múltiples worktrees, checkout del lado del host, secretos nativos del SSG, puertos virtuales a través de reconstrucciones del SSG — declara los servicios una vez en un [`Coastfile.shared_service_groups`](SHARED_SERVICE_GROUPS.md) y haz referencia a ellos desde el Coastfile del consumidor con `from_group = true`:

```toml
[shared_services.postgres]
from_group = true

# Optional per-consumer overrides:
inject = "env:DATABASE_URL"
# auto_create_db = false    # overrides the SSG service's default
```

La clave TOML (`postgres` en este ejemplo) debe coincidir con un servicio declarado en el `Coastfile.shared_service_groups` del proyecto. El SSG al que se hace referencia aquí es **siempre el SSG propio del proyecto consumidor** (llamado `<project>-ssg`, donde `<project>` es el `[coast].name` del consumidor).

### Campos prohibidos con `from_group = true`

Los siguientes campos se rechazan en tiempo de parseo porque el SSG es la única fuente de verdad:

- `image`
- `ports`
- `env`
- `volumes`

Cualquiera de estos junto con `from_group = true` produce:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

### Overrides permitidos por consumidor

- `inject` — la variable de entorno o la ruta de archivo a través de la cual se expone la cadena de conexión. Distintos Coastfiles consumidores pueden exponer el mismo Postgres del SSG bajo distintos nombres de variables de entorno.
- `auto_create_db` — si Coast debe crear una base de datos por instancia dentro de este servicio en el momento de `coast run`. Sobrescribe el valor `auto_create_db` propio del servicio del SSG.

### Error por servicio faltante

Si haces referencia a un nombre que no está declarado en el `Coastfile.shared_service_groups` del proyecto, `coast build` falla:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

### Cuándo elegir `from_group` en lugar de en línea

| Need | Inline | `from_group` |
|---|---|---|
| Single Coast project on this host, no secrets | Either works; inline is simpler | OK |
| Multiple worktrees / consumer instances of the **same** project sharing one Postgres | Works (siblings share one host container) | Works |
| **Two different Coast projects** on this host that each declare the same canonical port (e.g. both want Postgres on 5432) | Collides on host port; cannot run both concurrently | Required (each project's SSG owns its own inner Postgres without binding host 5432) |
| Want host-side `psql localhost:5432` via `coast ssg checkout` | -- | Required |
| Need build-time secret extraction for the service (`POSTGRES_PASSWORD` from a keychain, etc.) | -- | Required (see [SSG Secrets](../shared_service_groups/SECRETS.md)) |
| Stable consumer routing across rebuilds (virtual ports) | -- | Required (see [SSG Routing](../shared_service_groups/ROUTING.md)) |

Para la arquitectura completa del SSG, consulta [Grupos de Servicios Compartidos](../shared_service_groups/README.md). Para la experiencia del lado del consumidor, incluyendo arranque automático, detección de drift y consumidores remotos, consulta [Consuming](../shared_service_groups/CONSUMING.md).

---

## Ver también

- [Servicios compartidos (concepto)](../concepts_and_terminology/SHARED_SERVICES.md) -- arquitectura en tiempo de ejecución para ambas modalidades
- [Grupos de Servicios Compartidos](../shared_service_groups/README.md) -- descripción general del concepto SSG
- [Coastfile: Grupos de Servicios Compartidos](SHARED_SERVICE_GROUPS.md) -- esquema del Coastfile del lado del SSG
- [Consuming an SSG](../shared_service_groups/CONSUMING.md) -- guía detallada de la semántica de `from_group = true`

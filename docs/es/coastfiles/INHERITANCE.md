# Herencia, Tipos y Composición

Los Coastfiles admiten herencia (`extends`), composición de fragmentos (`includes`), eliminación de elementos (`[unset]`) y exclusión a nivel de compose (`[omit]`). En conjunto, estas funciones te permiten definir una configuración base una sola vez y crear variantes ligeras para distintos flujos de trabajo — ejecutores de pruebas, frontends livianos, stacks inicializados con snapshots — sin duplicar configuración.

Para una descripción más general de cómo los Coastfiles tipados encajan en el sistema de compilación, consulta [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) y [Builds](../concepts_and_terminology/BUILDS.md).

## Tipos de Coastfile

El Coastfile base siempre se llama `Coastfile`. Las variantes tipadas usan el patrón de nombres `Coastfile.{type}`:

- `Coastfile` — el tipo predeterminado
- `Coastfile.light` — tipo `light`
- `Coastfile.snap` — tipo `snap`
- `Coastfile.ci.minimal` — tipo `ci.minimal`

Cualquier Coastfile puede tener una extensión opcional `.toml` para el resaltado de sintaxis en el editor. El sufijo `.toml` se elimina antes de extraer el tipo:

- `Coastfile.toml` = `Coastfile` (tipo predeterminado)
- `Coastfile.light.toml` = `Coastfile.light` (tipo `light`)
- `Coastfile.ci.minimal.toml` = `Coastfile.ci.minimal` (tipo `ci.minimal`)

Si existen tanto la forma simple como la forma `.toml` (por ejemplo, `Coastfile` y `Coastfile.toml`), la variante `.toml` tiene prioridad.

Los nombres `Coastfile.default` y `"toml"` (como tipo) están reservados y no se permiten. Un punto final (`Coastfile.`) también es inválido.

Compila y ejecuta variantes tipadas con `--type`:

```
coast build --type light
coast run test-1 --type light
```

Cada tipo tiene su propio pool de compilación independiente. Una compilación con `--type light` no interfiere con las compilaciones predeterminadas.

## `extends`

Un Coastfile tipado puede heredar de un padre usando `extends` en la sección `[coast]`. El padre se analiza por completo primero, y luego los valores del hijo se superponen encima.

```toml
[coast]
extends = "Coastfile"
```

El valor es una ruta relativa al Coastfile padre, resuelta con respecto al directorio del hijo. Si la ruta exacta no existe, Coast también intentará añadir `.toml` — así que `extends = "Coastfile"` encontrará `Coastfile.toml` si solo la variante `.toml` existe en disco. Se admiten cadenas — un hijo puede extender un padre que a su vez extiende a un abuelo:

```
Coastfile                    (base)
  └─ Coastfile.light         (extiende Coastfile)
       └─ Coastfile.chain    (extiende Coastfile.light)
```

Las cadenas circulares (A extiende B extiende A, o A extiende A) se detectan y se rechazan.

### Semántica de combinación

Cuando un hijo extiende a un padre:

- **Campos escalares** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — el valor del hijo gana si está presente; de lo contrario se hereda del padre.
- **Mapas** (`[ports]`, `[egress]`) — se combinan por clave. Las claves del hijo sobrescriben las claves del padre con el mismo nombre; las claves exclusivas del padre se conservan.
- **Secciones con nombre** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — se combinan por nombre. Una entrada del hijo con el mismo nombre reemplaza por completo la entrada del padre; los nombres nuevos se añaden.
- **`[coast.setup]`**:
  - `packages` — unión sin duplicados (el hijo añade paquetes nuevos, se conservan los paquetes del padre)
  - `run` — los comandos del hijo se anexan después de los comandos del padre
  - `files` — se combinan por `path` (misma ruta = la entrada del hijo reemplaza la del padre)
- **`[inject]`** — las listas `env` y `files` se concatenan.
- **`[omit]`** — las listas `services` y `volumes` se concatenan.
- **`[assign]`** — se reemplaza por completo si está presente en el hijo (no se combina campo por campo).
- **`[agent_shell]`** — se reemplaza por completo si está presente en el hijo.

### Heredar el nombre del proyecto

Si el hijo no establece `name`, hereda el nombre del padre. Esto es normal para las variantes tipadas — son variantes del mismo proyecto:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — hereda el nombre "my-app"
[coast]
extends = "Coastfile"
autostart = false
```

Puedes sobrescribir `name` en el hijo si quieres que la variante aparezca como un proyecto separado:

```toml
[coast]
extends = "Coastfile"
name = "my-app-light"
```

## `includes`

El campo `includes` combina uno o más archivos de fragmentos TOML en el Coastfile antes de aplicar los valores propios del archivo. Esto es útil para extraer configuración compartida (como un conjunto de secretos o servidores MCP) en fragmentos reutilizables.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

Un fragmento incluido es un archivo TOML con la misma estructura de secciones que un Coastfile. Debe contener una sección `[coast]` (que puede estar vacía), pero no puede usar `extends` ni `includes` por sí mismo.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

Orden de combinación cuando están presentes tanto `extends` como `includes`:

1. Analizar el padre (mediante `extends`), recursivamente
2. Combinar cada fragmento incluido en orden
3. Aplicar los valores propios del archivo (que prevalecen sobre todo lo demás)

## `[unset]`

Elimina elementos con nombre de la configuración resuelta después de que toda la combinación se complete. Así es como un hijo elimina algo que heredó de su padre sin tener que redefinir toda la sección.

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

Campos admitidos:

- `secrets` — lista de nombres de secretos que se eliminarán
- `ports` — lista de nombres de puertos que se eliminarán
- `shared_services` — lista de nombres de servicios compartidos que se eliminarán
- `volumes` — lista de nombres de volúmenes que se eliminarán
- `mcp` — lista de nombres de servidores MCP que se eliminarán
- `mcp_clients` — lista de nombres de clientes MCP que se eliminarán
- `egress` — lista de nombres de salidas de red que se eliminarán
- `services` — lista de nombres de servicios simples que se eliminarán

`[unset]` se aplica después de que se resuelva toda la cadena de combinación de extends + includes. Elimina elementos por nombre del resultado final combinado.

## `[omit]`

Excluye servicios y volúmenes de compose del stack de Docker Compose que se ejecuta dentro de Coast. A diferencia de `[unset]` (que elimina configuración a nivel de Coastfile), `[omit]` le dice a Coast que excluya servicios o volúmenes específicos al ejecutar `docker compose up` dentro del contenedor DinD.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — nombres de servicios de compose que se excluirán de `docker compose up`
- **`volumes`** — nombres de volúmenes de compose que se excluirán

Esto es útil cuando tu `docker-compose.yml` define servicios que no necesitas en cada variante de Coast — stacks de monitoreo, proxies inversos, herramientas de administración. En lugar de mantener múltiples archivos de compose, usas un solo archivo de compose y eliminas lo que no necesitas por variante.

Cuando un hijo extiende a un padre, las listas de `[omit]` se concatenan — el hijo añade elementos a la lista de exclusión del padre.

## Ejemplos

### Variante ligera de pruebas

Extiende el Coastfile base, desactiva el autoinicio, elimina servicios compartidos y ejecuta bases de datos aisladas por instancia:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis", "mongodb"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
service = "test-redis"
mount = "/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
migrations = "rebuild"
```

### Variante inicializada con snapshot

Elimina los servicios compartidos de la base y los reemplaza con volúmenes aislados inicializados con snapshot:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

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

### Variante tipada con servicios compartidos extra e includes

Extiende la base, añade MongoDB e incorpora secretos extra desde un fragmento:

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
mongodb = 37017

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "dev", MONGO_INITDB_ROOT_PASSWORD = "dev" }

[omit]
services = ["debug-tools"]
```

### Cadena de herencia multinivel

Tres niveles de profundidad: base -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

La configuración resuelta comienza con el `Coastfile` base, combina `Coastfile.light` encima y luego combina `Coastfile.chain` encima de eso. Los comandos `run` de setup de los tres niveles se concatenan en orden. Los `packages` de setup se desduplican en todos los niveles.

### Excluir servicios de un stack de compose grande

Elimina servicios de `docker-compose.yml` que no son necesarios para desarrollo:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["backend-debug", "backend-debug-test", "asynqmon", "postgres-keycloak", "keycloak", "redash-db-init", "redash-init", "redash", "redash-scheduler", "redash-worker", "langfuse-db-init", "langfuse", "nginx-proxy"]
volumes = ["keycloak-db-data"]

[ports]
web = 3000
backend = 8080
```

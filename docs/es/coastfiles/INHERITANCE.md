# Herencia, tipos y composición

Los Coastfiles admiten herencia (`extends`), composición de fragmentos (`includes`), eliminación de elementos (`[unset]`) y depuración a nivel de compose (`[omit]`). En conjunto, esto te permite definir una configuración base una sola vez y crear variantes ligeras para distintos flujos de trabajo — ejecutores de pruebas, frontends livianos, stacks sembrados con snapshots — sin duplicar configuración.

Para una visión general de alto nivel de cómo los Coastfiles tipados encajan en el sistema de build, consulta [Coastfile Types](../concepts_and_terminology/COASTFILE_TYPES.md) y [Builds](../concepts_and_terminology/BUILDS.md).

## Tipos de Coastfile

El Coastfile base siempre se llama `Coastfile`. Las variantes tipadas usan el patrón de nombres `Coastfile.{type}`:

- `Coastfile` — el tipo predeterminado
- `Coastfile.light` — tipo `light`
- `Coastfile.snap` — tipo `snap`
- `Coastfile.ci.minimal` — tipo `ci.minimal`

El nombre `Coastfile.default` está reservado y no se permite. Un punto final (`Coastfile.`) también es inválido.

Compila y ejecuta variantes tipadas con `--type`:

```
coast build --type light
coast run test-1 --type light
```

Cada tipo tiene su propio pool de compilación independiente. Una compilación con `--type light` no interfiere con las compilaciones predeterminadas.

## `extends`

Un Coastfile tipado puede heredar de un padre usando `extends` en la sección `[coast]`. El padre se analiza completamente primero y luego los valores del hijo se superponen encima.

```toml
[coast]
extends = "Coastfile"
```

El valor es una ruta relativa al Coastfile padre, resuelta con respecto al directorio del hijo. Se admiten cadenas — un hijo puede extender un padre que a su vez extiende a un abuelo:

```
Coastfile                    (base)
  └─ Coastfile.light         (extends Coastfile)
       └─ Coastfile.chain    (extends Coastfile.light)
```

Las cadenas circulares (A extiende B extiende A, o A extiende A) se detectan y se rechazan.

### Semántica de fusión

Cuando un hijo extiende a un padre:

- **Campos escalares** (`name`, `runtime`, `compose`, `root`, `worktree_dir`, `autostart`, `primary_port`) — el valor del hijo gana si está presente; de lo contrario se hereda del padre.
- **Mapas** (`[ports]`, `[egress]`) — se fusionan por clave. Las claves del hijo sobrescriben las claves del padre con el mismo nombre; las claves solo del padre se conservan.
- **Secciones con nombre** (`[secrets.*]`, `[volumes.*]`, `[shared_services.*]`, `[mcp.*]`, `[mcp_clients.*]`, `[services.*]`) — se fusionan por nombre. Una entrada del hijo con el mismo nombre reemplaza completamente la entrada del padre; se agregan los nombres nuevos.
- **`[coast.setup]`**:
  - `packages` — unión con deduplicación (el hijo añade paquetes nuevos, los paquetes del padre se mantienen)
  - `run` — los comandos del hijo se anexan después de los comandos del padre
  - `files` — se fusionan por `path` (misma ruta = la entrada del hijo reemplaza la del padre)
- **`[inject]`** — las listas `env` y `files` se concatenan.
- **`[omit]`** — las listas `services` y `volumes` se concatenan.
- **`[assign]`** — se reemplaza por completo si está presente en el hijo (no se fusiona campo por campo).
- **`[agent_shell]`** — se reemplaza por completo si está presente en el hijo.

### Heredar el nombre del proyecto

Si el hijo no establece `name`, hereda el nombre del padre. Esto es normal para variantes tipadas — son variantes del mismo proyecto:

```toml
# Coastfile
[coast]
name = "my-app"
```

```toml
# Coastfile.light — inherits name "my-app"
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

El campo `includes` fusiona uno o más archivos de fragmentos TOML en el Coastfile antes de que se apliquen los valores del propio archivo. Esto es útil para extraer configuración compartida (como un conjunto de secretos o servidores MCP) en fragmentos reutilizables.

```toml
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]
```

Un fragmento incluido es un archivo TOML con la misma estructura de secciones que un Coastfile. Debe contener una sección `[coast]` (que puede estar vacía) pero no puede usar `extends` ni `includes` por sí mismo.

```toml
# extra-secrets.toml
[coast]

[secrets.mongo_uri]
extractor = "env"
var = "MONGO_URI"
inject = "env:MONGO_URI"
```

Orden de fusión cuando están presentes tanto `extends` como `includes`:

1. Analizar el padre (vía `extends`), recursivamente
2. Fusionar cada fragmento incluido en orden
3. Aplicar los valores propios del archivo (que ganan sobre todo lo demás)

## `[unset]`

Elimina elementos con nombre de la configuración resuelta después de que toda la fusión haya finalizado. Así es como un hijo elimina algo que heredó de su padre sin tener que redefinir toda la sección.

```toml
[unset]
secrets = ["db_password"]
shared_services = ["postgres", "redis"]
ports = ["postgres", "redis"]
```

Campos compatibles:

- `secrets` — lista de nombres de secretos a eliminar
- `ports` — lista de nombres de puertos a eliminar
- `shared_services` — lista de nombres de servicios compartidos a eliminar
- `volumes` — lista de nombres de volúmenes a eliminar
- `mcp` — lista de nombres de servidores MCP a eliminar
- `mcp_clients` — lista de nombres de clientes MCP a eliminar
- `egress` — lista de nombres de egresos a eliminar
- `services` — lista de nombres de servicios simples a eliminar

`[unset]` se aplica después de que se resuelva la cadena completa de fusión de extends + includes. Elimina elementos por nombre del resultado final fusionado.

## `[omit]`

Elimina servicios y volúmenes de compose del stack de Docker Compose que se ejecuta dentro de Coast. A diferencia de `[unset]` (que elimina configuración a nivel de Coastfile), `[omit]` le dice a Coast que excluya servicios o volúmenes específicos al ejecutar `docker compose up` dentro del contenedor DinD.

```toml
[omit]
services = ["monitoring", "debug-tools", "nginx-proxy"]
volumes = ["keycloak-db-data"]
```

- **`services`** — nombres de servicios de compose a excluir de `docker compose up`
- **`volumes`** — nombres de volúmenes de compose a excluir

Esto es útil cuando tu `docker-compose.yml` define servicios que no necesitas en cada variante de Coast — stacks de monitoreo, proxies inversos, herramientas de administración. En lugar de mantener múltiples archivos de compose, usas un único archivo de compose y eliminas lo que no necesitas por variante.

Cuando un hijo extiende a un padre, las listas de `[omit]` se concatenan — el hijo añade a la lista de omisiones del padre.

## Ejemplos

### Variante de pruebas ligera

Extiende el Coastfile base, deshabilita el autoinicio, elimina servicios compartidos y ejecuta bases de datos aisladas por instancia:

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

### Variante sembrada con snapshot

Elimina los servicios compartidos de la base y los reemplaza con volúmenes aislados sembrados con snapshot:

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

### Variante tipada con servicios compartidos adicionales e includes

Extiende la base, añade MongoDB e incorpora secretos adicionales desde un fragmento:

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

### Cadena de herencia de varios niveles

Tres niveles: base -> light -> chain.

```toml
# Coastfile.chain
[coast]
extends = "Coastfile.light"

[coast.setup]
run = ["echo 'chain setup appended'"]

[ports]
debug = 39999
```

La configuración resuelta comienza con el `Coastfile` base, fusiona `Coastfile.light` encima y luego fusiona `Coastfile.chain` encima de eso. Los comandos `run` de setup de los tres niveles se concatenan en orden. Los `packages` de setup se deduplican a través de todos los niveles.

### Omitir servicios de un gran stack de compose

Elimina servicios de `docker-compose.yml` que no se necesitan para desarrollo:

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

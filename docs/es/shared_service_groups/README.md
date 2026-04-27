# Grupos de Servicios Compartidos

Un Grupo de Servicios Compartidos (SSG) es un contenedor Docker-in-Docker que ejecuta los servicios de infraestructura de tu proyecto -- Postgres, Redis, MongoDB, cualquier cosa que de otro modo pondrías bajo `[shared_services]` -- en un solo lugar, por separado de las instancias de [Coast](../concepts_and_terminology/COASTS.md) que lo consumen. Cada proyecto Coast obtiene su propio SSG, llamado `<project>-ssg`, declarado por un `Coastfile.shared_service_groups` al mismo nivel que el `Coastfile` del proyecto.

Cada instancia consumidora (`dev-1`, `dev-2`, ...) se conecta al SSG de su proyecto mediante puertos virtuales estables, por lo que las reconstrucciones del SSG no alteran a los consumidores. Dentro de cada Coast, el contrato no cambia: `postgres:5432` resuelve a tu Postgres compartido, el código de la aplicación no sabe que hay nada especial.

## Por qué un SSG

El patrón original de [Servicios Compartidos](../concepts_and_terminology/SHARED_SERVICES.md) inicia un contenedor de infraestructura en el daemon Docker del host y lo comparte entre cada instancia consumidora del proyecto. Eso funciona bien para un proyecto. El problema comienza cuando tienes **dos proyectos diferentes** que cada uno declara un Postgres en `5432`: ambos proyectos intentan enlazar el mismo puerto del host y el segundo falla.

```text
Without an SSG (cross-project host-port collision):

Host Docker daemon
+-- cg-coasts-postgres            (project "cg" binds host :5432)
+-- filemap-coasts-postgres       (project "filemap" tries :5432 -- FAILS)
+-- cg-coasts-dev-1               --> cg-coasts-postgres
+-- cg-coasts-dev-2               --> cg-coasts-postgres   (siblings share fine)
```

Los SSG resuelven esto elevando la infraestructura de cada proyecto a su propio DinD. Postgres sigue escuchando en el `:5432` canónico -- pero dentro del SSG, no en el host. El contenedor SSG se publica en un puerto dinámico arbitrario del host, y un socat de puerto virtual administrado por el daemon (en la banda `42000-43000`) puentea el tráfico de los consumidores hacia él. Dos proyectos pueden tener cada uno un Postgres en el 5432 canónico porque ninguno enlaza el 5432 del host:

```text
With an SSG (per project, no cross-project collision):

Host Docker daemon
+-- cg-ssg                        (project "cg" -- DinD)
|     +-- postgres                (inner :5432, host dyn 54201, vport 42000)
|     +-- redis                   (inner :6379, host dyn 54202, vport 42001)
+-- filemap-ssg                   (project "filemap" -- DinD, no collision)
|     +-- postgres                (inner :5432, host dyn 54250, vport 42002)
|     +-- redis                   (inner :6379, host dyn 54251, vport 42003)
+-- cg-coasts-dev-1               --> hg-internal:42000 --> cg-ssg postgres
+-- cg-coasts-dev-2               --> hg-internal:42000 --> cg-ssg postgres
+-- filemap-coasts-dev-1          --> hg-internal:42002 --> filemap-ssg postgres
```

El SSG de cada proyecto posee sus propios datos, sus propias versiones de imagen y sus propios secretos. Los dos nunca comparten estado, nunca compiten por puertos y nunca ven los datos del otro. Dentro de cada Coast consumidor, el contrato no cambia: el código de la app se conecta a `postgres:5432` y obtiene el Postgres de su propio proyecto -- la capa de enrutamiento (ver [Routing](ROUTING.md)) hace el resto.

## Inicio Rápido

Un `Coastfile.shared_service_groups` está al mismo nivel que el `Coastfile` del proyecto. El nombre del proyecto viene de `[coast].name` en el Coastfile normal -- no lo repites.

```toml
# Coastfile.shared_service_groups
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_DB = "app_dev" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]

# Optional: extract secrets from your environment, keychain, or 1Password
# at build time and inject them into the SSG at run time. See SECRETS.md.
[secrets.pg_password]
extractor = "env"
inject = "env:POSTGRES_PASSWORD"
var = "MY_PG_PASSWORD"
```

Constrúyelo y ejecútalo:

```bash
coast ssg build       # parse, pull images, extract secrets, write artifact
coast ssg run         # start <project>-ssg, materialize secrets, compose up
coast ssg ps          # show service status
```

Apunta un Coast consumidor hacia él:

```toml
# Coastfile in the same project
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
inject = "env:DATABASE_URL"

[shared_services.redis]
from_group = true
```

Luego `coast build && coast run dev-1`. El SSG se inicia automáticamente si aún no está en ejecución. Dentro del contenedor de la app de `dev-1`, `postgres:5432` resuelve al Postgres del SSG y `$DATABASE_URL` se establece en una cadena de conexión canónica.

## Referencia

| Page | What it covers |
|---|---|
| [Building](BUILDING.md) | `coast ssg build` de extremo a extremo, el diseño del artefacto por proyecto, extracción de secretos, las reglas de descubrimiento de `Coastfile.shared_service_groups`, y cómo fijar un proyecto a una compilación específica |
| [Lifecycle](LIFECYCLE.md) | `run` / `start` / `stop` / `restart` / `rm` / `ps` / `logs` / `exec`, el contenedor `<project>-ssg` por proyecto, inicio automático en `coast run`, y `coast ssg ls` para listados entre proyectos |
| [Routing](ROUTING.md) | Puertos canónicos / dinámicos / virtuales, la capa socat del host, la cadena completa salto por salto desde la app hasta el servicio interno, y túneles simétricos para consumidores remotos |
| [Volumes](VOLUMES.md) | Montajes bind del host, rutas simétricas, volúmenes nombrados internos, permisos, el comando `coast ssg doctor`, y migrar un volumen existente del host al SSG |
| [Consuming](CONSUMING.md) | `from_group = true`, campos permitidos y prohibidos, detección de conflictos, `auto_create_db`, `inject`, y consumidores remotos |
| [Secrets](SECRETS.md) | `[secrets.<name>]` en el Coastfile del SSG, el pipeline de extractores en tiempo de compilación, inyección en tiempo de ejecución mediante `compose.override.yml`, y el verbo `coast ssg secrets clear` |
| [Checkout](CHECKOUT.md) | `coast ssg checkout` / `uncheckout` para enlazar los puertos canónicos del SSG en el host de modo que cualquier cosa en tu host (psql, redis-cli, IDE) pueda alcanzarlos |
| [CLI](CLI.md) | Resumen en una línea de cada subcomando de `coast ssg` |

## Ver También

- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- el patrón en línea por instancia que SSG generaliza
- [Shared Services Coastfile reference](../coastfiles/SHARED_SERVICES.md) -- sintaxis TOML del lado consumidor, incluyendo `from_group`
- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- esquema completo para `Coastfile.shared_service_groups`
- [Ports](../concepts_and_terminology/PORTS.md) -- puertos canónicos vs dinámicos

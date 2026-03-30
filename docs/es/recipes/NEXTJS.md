# Aplicación Next.js

Esta receta es para una aplicación Next.js respaldada por Postgres y Redis, con workers en segundo plano opcionales o servicios complementarios. La pila ejecuta Next.js como un [servicio bare](../concepts_and_terminology/BARE_SERVICES.md) con Turbopack para HMR rápido, mientras que Postgres y Redis se ejecutan como [servicios compartidos](../concepts_and_terminology/SHARED_SERVICES.md) en el host para que cada instancia de Coast comparta los mismos datos.

Este patrón funciona bien cuando:

- Tu proyecto usa Next.js con Turbopack en desarrollo
- Tienes una capa de base de datos y caché (Postgres, Redis) que respalda la aplicación
- Quieres múltiples instancias de Coast ejecutándose en paralelo sin configuración de base de datos por instancia
- Usas bibliotecas de autenticación como NextAuth que incrustan URLs de callback en las respuestas

## The Complete Coastfile

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]

[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]

# --- Bare services: Next.js and background worker ---

[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]

[services.worker]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev:worker"
restart = "on-failure"
cache = ["node_modules"]

# --- Shared services: Postgres and Redis on the host ---

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]

# --- Secrets: connection strings for bare services ---

[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"

# --- Ports ---

[ports]
web = 3000
postgres = 5432
redis = 6379

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

## Proyecto y configuración

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]
```

**`private_paths`** es crítico para Next.js. Turbopack crea un archivo de bloqueo en `.next/dev/lock` al iniciar. Sin `private_paths`, una segunda instancia de Coast en la misma rama ve el bloqueo y se niega a iniciar. Con ello, cada instancia obtiene su propio directorio `.next` aislado mediante un montaje overlay por instancia. Consulta [Private Paths](../concepts_and_terminology/PRIVATE_PATHS.md).

**`worktree_dir`** enumera los directorios donde viven los git worktrees. Si usas múltiples agentes de programación (Claude Code, Cursor, Codex), cada uno puede crear worktrees en ubicaciones diferentes. Enumerarlos todos permite a Coast descubrir y asignar worktrees sin importar qué herramienta los creó.

```toml
[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]
```

La sección de configuración instala paquetes del sistema y herramientas necesarias para los servicios bare. `corepack enable` activa yarn o pnpm según el campo `packageManager` del proyecto. Estos se ejecutan en tiempo de compilación dentro de la imagen de Coast, no al inicio de la instancia.

## Servicios bare

```toml
[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]
```

**Instalaciones condicionales:** El patrón `test -f node_modules/.yarn-state.yml || make yarn` omite la instalación de dependencias si `node_modules` ya existe. Esto hace que los cambios de rama sean rápidos cuando las dependencias no han cambiado. Consulta [Bare Service Optimization](../concepts_and_terminology/BARE_SERVICE_OPTIMIZATION.md).

**`cache`:** Conserva `node_modules` entre cambios de worktree para que `yarn install` se ejecute de forma incremental en lugar de desde cero.

**`AUTH_URL` con puerto dinámico:** Las aplicaciones Next.js que usan NextAuth (u otras bibliotecas de autenticación similares) incrustan URLs de callback en las respuestas. Dentro de Coast, Next.js escucha en el puerto 3000, pero el puerto del lado del host es dinámico. Coast inyecta `WEB_DYNAMIC_PORT` en el entorno del contenedor automáticamente (derivado de la clave `web` en `[ports]`). El fallback `:-3000` significa que el mismo comando funciona fuera de Coast. Consulta [Dynamic Port Environment Variables](../concepts_and_terminology/DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md).

**`host.docker.internal`:** Los servicios bare no pueden alcanzar los servicios compartidos a través de `localhost` porque los servicios compartidos se ejecutan en el daemon Docker del host. `host.docker.internal` resuelve al host desde dentro del contenedor de Coast.

## Servicios compartidos

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]
```

Postgres y Redis se ejecutan en el daemon Docker del host como [servicios compartidos](../concepts_and_terminology/SHARED_SERVICES.md). Cada instancia de Coast se conecta a las mismas bases de datos, por lo que usuarios, sesiones y datos se comparten entre instancias. Esto evita el problema de tener que registrarse por separado en cada instancia.

Si tu proyecto ya tiene un `docker-compose.yml` con Postgres y Redis, puedes usar `compose` en su lugar y establecer la estrategia de volumen en `shared`. Los servicios compartidos son más simples para Coastfiles de servicios bare porque no hay un archivo compose que gestionar.

## Secrets

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"
```

Estos inyectan `DATABASE_URL` y `REDIS_URL` en el entorno del contenedor de Coast en tiempo de compilación. Las cadenas de conexión apuntan a los servicios compartidos mediante `host.docker.internal`.

El extractor `command` ejecuta un comando de shell y captura stdout. Aquí simplemente hace echo de una cadena estática, pero podrías usarlo para leer desde un vault, ejecutar una herramienta CLI o calcular un valor dinámicamente.

Ten en cuenta que los campos `command` de los servicios bare también establecen estas variables inline. Los valores inline tienen prioridad, pero los secrets inyectados sirven como valores predeterminados para los pasos de `install` y las sesiones de `coast exec`.

## Estrategias de assign

```toml
[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

**`default = "none"`** deja los servicios compartidos y la infraestructura intactos al cambiar de rama. Solo los servicios que dependen del código obtienen una estrategia de assign.

**`hot` para Next.js y workers:** Next.js con Turbopack tiene reemplazo de módulos en caliente incorporado. Cuando Coast vuelve a montar `/workspace` en el nuevo worktree, Turbopack detecta los cambios de archivos y recompila automáticamente. No hace falta reiniciar el proceso. Los workers en segundo plano que usan `tsc --watch` o `nodemon` también detectan los cambios a través de sus file watchers.

**`rebuild_triggers`:** Si `package.json` o `yarn.lock` cambiaron entre ramas, los comandos `install` del servicio se vuelven a ejecutar antes de que el servicio se reinicie. Esto asegura que las dependencias estén actualizadas después de un cambio de rama que agregó o eliminó paquetes.

**`exclude_paths`:** Acelera el bootstrap inicial del worktree omitiendo directorios que los servicios no necesitan. La documentación, las configuraciones de CI y los scripts son seguros de excluir.

## Adaptación de esta receta

**Sin worker en segundo plano:** Elimina la sección `[services.worker]` y su entrada de assign. El resto del Coastfile funciona sin cambios.

**Monorepo con múltiples aplicaciones Next.js:** Agrega una entrada `private_paths` para el directorio `.next` de cada aplicación. Cada servicio bare obtiene su propia sección `[services.*]` con el `command` y el `port` adecuados.

**pnpm en lugar de yarn:** Reemplaza `make yarn` con tu comando de instalación de pnpm. Ajusta el campo `cache` si pnpm almacena dependencias en una ubicación diferente (por ejemplo, `.pnpm-store`).

**Sin servicios compartidos:** Si prefieres bases de datos por instancia, elimina las secciones `[shared_services]` y `[secrets]`. Agrega Postgres y Redis a un `docker-compose.yml`, establece `compose` en la sección `[coast]` y usa [volume strategies](../coastfiles/VOLUMES.md) para controlar el aislamiento. Usa `strategy = "isolated"` para datos por instancia o `strategy = "shared"` para datos compartidos.

**Proveedores de autenticación adicionales:** Si tu biblioteca de autenticación usa variables de entorno distintas de `AUTH_URL` para las URLs de callback, aplica el mismo patrón `${WEB_DYNAMIC_PORT:-3000}` a esas variables en el comando del servicio.

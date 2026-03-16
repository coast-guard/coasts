# Coastfiles

Un Coastfile es un archivo de configuración TOML que vive en la raíz de tu proyecto. Le indica a Coast todo lo que necesita saber para construir y ejecutar entornos de desarrollo aislados para ese proyecto: qué servicios ejecutar, qué puertos reenviar, cómo manejar los datos y cómo administrar los secretos.

Cada proyecto de Coast necesita al menos un Coastfile. El archivo siempre se llama `Coastfile` (C mayúscula, sin extensión). Si necesitas variantes para diferentes flujos de trabajo, creas Coastfiles tipados como `Coastfile.light` o `Coastfile.snap` que [heredan del base](INHERITANCE.md).

Para una comprensión más profunda de cómo se relacionan los Coastfiles con el resto de Coast, consulta [Coasts](../concepts_and_terminology/COASTS.md) y [Builds](../concepts_and_terminology/BUILDS.md).

## Quickstart

El Coastfile más pequeño posible:

```toml
[coast]
name = "my-app"
```

Esto te da un contenedor DinD al que puedes entrar con `coast exec`. La mayoría de los proyectos querrán ya sea una referencia `compose` o [servicios bare](SERVICES.md):

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

O sin compose, usando servicios bare:

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[ports]
web = 3000
```

Ejecuta `coast build` y luego `coast run dev-1` y tendrás un entorno aislado.

## Example Coastfiles

### Proyecto simple con servicios bare

Una app de Next.js sin archivo compose. Coast instala Node, ejecuta `npm install` e inicia el servidor de desarrollo directamente.

```toml
[coast]
name = "my-crm"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Proyecto full-stack con compose

Un proyecto multiservicio con bases de datos compartidas, secretos, estrategias de volúmenes y configuración personalizada.

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "curl", "git", "bash", "ca-certificates", "wget"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]

[ports]
web = 3000
backend = 8080
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[omit]
services = ["monitoring", "admin-panel", "nginx-proxy"]

[assign]
default = "none"
[assign.services]
backend = "hot"
web = "hot"
```

### Variante ligera para pruebas (herencia)

Extiende el Coastfile base, pero lo reduce a solo lo necesario para ejecutar pruebas del backend. Sin puertos, sin servicios compartidos, bases de datos aisladas.

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
```

### Variante inicializada con snapshot

Cada instancia de coast comienza con una copia de los volúmenes de base de datos existentes del host y luego diverge de forma independiente.

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

## Conventions

- El archivo debe llamarse `Coastfile` (C mayúscula, sin extensión) y vivir en la raíz del proyecto.
- Las variantes tipadas usan el patrón `Coastfile.{type}` — por ejemplo `Coastfile.light`, `Coastfile.snap`. Consulta [Inheritance and Types](INHERITANCE.md).
- El nombre reservado `Coastfile.default` no está permitido.
- Se usa sintaxis TOML en todo el documento. Todos los encabezados de sección usan `[brackets]` y las entradas con nombre usan `[section.name]` (no array-of-tables).
- No puedes usar tanto `compose` como `[services]` en el mismo Coastfile — elige uno.
- Las rutas relativas (para `compose`, `root`, etc.) se resuelven contra el directorio padre del Coastfile.

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | Nombre, ruta de compose, runtime, worktree dir, configuración del contenedor |
| [Worktree Directories](WORKTREE_DIR.md) | `worktree_dir`, `default_worktree_dir` | Directorios worktree locales y externos, rutas con tilde, integración con Codex/Claude |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | Reenvío de puertos, declaraciones de egress, puerto primario |
| [Volumes](VOLUMES.md) | `[volumes.*]` | Estrategias de volúmenes aislados, compartidos e inicializados con snapshot |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | Bases de datos a nivel de host y servicios de infraestructura |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | Extracción, inyección y reenvío de secretos desde env/archivo del host |
| [Bare Services](SERVICES.md) | `[services.*]` | Ejecutar procesos directamente sin Docker Compose |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | Entornos TUI de agente en contenedores |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | Servidores MCP internos y proxyados desde el host, conectores de cliente |
| [Assign](ASSIGN.md) | `[assign]` | Comportamiento al cambiar de rama por servicio |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | Coastfiles tipados, composición y sobrescrituras |

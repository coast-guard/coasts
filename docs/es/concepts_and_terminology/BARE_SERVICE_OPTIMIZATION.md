# Optimización de Bare Service

Los [bare services](BARE_SERVICES.md) se ejecutan como procesos simples dentro del contenedor de Coast. Sin capas de Docker ni cachés de imágenes, el rendimiento de inicio y de cambio de rama depende de cómo estructures tus comandos de `install`, el almacenamiento en caché y las estrategias de assign.

## Comandos de instalación rápidos

El campo `install` se ejecuta antes de que el servicio se inicie y nuevamente en cada `coast assign`. Si `install` ejecuta incondicionalmente `make` o `yarn install`, cada cambio de rama paga el costo completo de instalación incluso cuando nada cambió.

**Usa comprobaciones condicionales para omitir trabajo cuando sea posible:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

La protección `test -f` omite la instalación si `node_modules` ya existe. En la primera ejecución o después de una falla de caché, ejecuta la instalación completa. En asignaciones posteriores donde las dependencias no han cambiado, se completa al instante.

Para binarios compilados, verifica si la salida existe:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## Directorios de caché entre worktrees

Cuando Coast cambia una instancia de bare-service a un nuevo worktree, el montaje de `/workspace` cambia a un directorio diferente. Los artefactos de compilación como `node_modules` o los binarios compilados se quedan atrás en el worktree anterior. El campo `cache` le indica a Coast que conserve directorios específicos entre cambios:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

Los directorios en caché se respaldan antes del remontaje del worktree y se restauran después. Esto significa que `yarn install` se ejecuta de forma incremental en lugar de desde cero, y los binarios compilados sobreviven a los cambios de rama.

## Aísla directorios por instancia con private_paths

Algunas herramientas crean directorios en el workspace que contienen estado por proceso: archivos de bloqueo, cachés de compilación o archivos PID. Cuando varias instancias de Coast comparten el mismo workspace (misma rama, sin worktree), estos directorios entran en conflicto.

El ejemplo clásico es Next.js, que toma un bloqueo en `.next/dev/lock` al iniciarse. Una segunda instancia de Coast ve el bloqueo y se niega a iniciarse.

`private_paths` le da a cada instancia su propio directorio aislado para las rutas especificadas:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

Cada instancia obtiene un montaje overlay por instancia en esa ruta. Los archivos de bloqueo, las cachés de compilación y el estado de Turbopack quedan completamente aislados. No se necesitan cambios de código.

Usa `private_paths` para cualquier directorio donde instancias concurrentes escribiendo en los mismos archivos causen problemas: `.next`, `.turbo`, `.parcel-cache`, archivos PID o bases de datos SQLite.

## Conectarse a servicios compartidos

Cuando usas [shared services](SHARED_SERVICES.md) para bases de datos o cachés, los contenedores compartidos se ejecutan en el daemon de Docker del host, no dentro de Coast. Los bare services que se ejecutan dentro de Coast no pueden alcanzarlos a través de `localhost`.

Usa `host.docker.internal` en su lugar:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

También puedes usar [secrets](../coastfiles/SECRETS.md) para inyectar cadenas de conexión como variables de entorno:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Los servicios de Compose dentro de Coast no tienen este problema. Coast enruta automáticamente los nombres de host de los shared services a través de una red puente para contenedores de compose. Esto solo afecta a los bare services.

## Variables de entorno en línea

Los comandos de bare service heredan variables de entorno del contenedor de Coast, incluyendo cualquier cosa establecida mediante archivos `.env`, secrets e inject. Pero a veces necesitas sobrescribir una variable específica para un único servicio sin cambiar archivos de configuración compartidos.

Prefija el comando con asignaciones en línea:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

Las variables en línea tienen prioridad sobre todo lo demás. Esto es útil para:

- Establecer `AUTH_URL` al [puerto dinámico](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) para que las redirecciones de autenticación funcionen en instancias no checked-out
- Sobrescribir `DATABASE_URL` para que apunte a un shared service mediante `host.docker.internal`
- Establecer flags específicos del servicio sin modificar archivos `.env` compartidos en el workspace

## Estrategias de assign para bare services

Elige la [estrategia de assign](../coastfiles/ASSIGN.md) adecuada según cómo cada servicio incorpore los cambios de código:

| Strategy | When to use | Examples |
|---|---|---|
| `hot` | El servicio tiene un observador de archivos que detecta cambios automáticamente después del remontaje del worktree | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | El servicio carga el código al iniciarse y no observa cambios | Binarios Go compilados, Rails, servidores Java |
| `none` | El servicio no depende del código del workspace o usa un índice separado | Servidores de base de datos, Redis, índices de búsqueda |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

Establecer el valor predeterminado en `none` significa que los servicios de infraestructura nunca se tocan al cambiar de rama. Solo los servicios a los que les importan los cambios de código se reinician o dependen de la recarga en caliente.

## Ver también

- [Bare Services](BARE_SERVICES.md) - la referencia completa de bare services
- [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) - ajuste general del rendimiento, incluyendo `exclude_paths` y `rebuild_triggers`
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - uso de `WEB_DYNAMIC_PORT` y variables relacionadas en comandos

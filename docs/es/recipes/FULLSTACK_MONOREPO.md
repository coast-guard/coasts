# Monorepo Full-Stack

Esta receta es para un monorepo grande con múltiples aplicaciones web respaldadas por una base de datos y una capa de caché compartidas. El stack usa Docker Compose para los servicios backend pesados (Rails, Sidekiq, SSR) y ejecuta servidores de desarrollo Vite como servicios bare en el host DinD. Postgres y Redis se ejecutan como servicios compartidos en el daemon de Docker del host para que cada instancia de Coast hable con la misma infraestructura sin duplicarla.

Este patrón funciona bien cuando:

- Tu monorepo contiene varias apps que comparten una base de datos
- Quieres instancias de Coast ligeras que no ejecuten cada una su propio Postgres y Redis
- Tus servidores de desarrollo del frontend necesitan ser accesibles desde dentro de contenedores de compose vía `host.docker.internal`
- Tienes integraciones MCP del lado del host que se conectan a `localhost:5432` y quieres que sigan funcionando sin cambios

## El Coastfile Completo

Aquí está el Coastfile completo. Cada sección se explica en detalle abajo.

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]

[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"

# --- Bare services: Vite dev servers on the DinD host ---

[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"

# --- Shared services: Postgres and Redis on the host daemon ---

[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]

# --- Volumes: shared caches across all instances ---

[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"

# --- Secrets and injection ---

[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

## Proyecto y Compose

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]
```

El campo `compose` apunta a tu archivo de Docker Compose existente. Coast ejecuta `docker compose up -d` dentro del contenedor DinD en `coast run`, así que tus servicios backend (servidores Rails, workers Sidekiq, procesos SSR) se inician automáticamente.

`[coast.setup]` instala paquetes en el propio host DinD — no dentro de tus contenedores de compose. Estos son necesarios para los servicios bare (servidores de desarrollo Vite) que se ejecutan directamente en el host. Tus servicios de compose obtienen sus runtimes de sus Dockerfiles como siempre.

## Servicios Compartidos

```toml
[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]
```

Postgres y Redis se declaran como [servicios compartidos](../concepts_and_terminology/SHARED_SERVICES.md) en lugar de ejecutarse dentro de cada Coast. Esto significa que se ejecutan en el daemon de Docker del host, y cada instancia de Coast se conecta a ellos a través de una red bridge.

**¿Por qué servicios compartidos en lugar de bases de datos internas de compose?**

- **Instancias más ligeras.** Cada Coast evita levantar sus propios contenedores de Postgres y Redis, lo que ahorra memoria y tiempo de arranque.
- **Reutilización de volúmenes del host.** El campo `volumes` referencia tus volúmenes de Docker existentes (los creados por tu `docker-compose up` local). Todos los datos que ya tienes están disponibles de inmediato — sin seeding, sin volver a ejecutar migraciones.
- **Compatibilidad con MCP.** Si tienes herramientas MCP de base de datos en tu host conectándose a `localhost:5432`, siguen funcionando porque el Postgres compartido está en el host en ese mismo puerto. No se requiere reconfiguración.

**La desventaja:** no hay aislamiento de datos entre instancias de Coast. Cada instancia lee y escribe en la misma base de datos. Si tu flujo de trabajo necesita bases de datos por instancia, usa [estrategias de volumen](../concepts_and_terminology/VOLUMES.md) con `strategy = "isolated"` en su lugar, o usa `auto_create_db = true` en el servicio compartido para obtener una base de datos por instancia dentro del Postgres compartido. Consulta la [referencia de Coastfile para Servicios Compartidos](../coastfiles/SHARED_SERVICES.md) para más detalles.

**El nombre de los volúmenes importa.** Los nombres de volumen (`infra_postgres`, `infra_redis`) deben coincidir con los volúmenes que ya existen en tu host por haber ejecutado `docker-compose up` localmente. Si no coinciden, el servicio compartido arrancará con un volumen vacío. Ejecuta `docker volume ls` para comprobar los nombres de volumen existentes antes de escribir esta sección.

## Servicios Bare

```toml
[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"
```

Los servidores de desarrollo Vite se definen como [servicios bare](../concepts_and_terminology/BARE_SERVICES.md) — procesos simples que se ejecutan directamente en el host DinD, fuera de Docker Compose. Este es el patrón de [tipos de servicio mixtos](../concepts_and_terminology/MIXED_SERVICE_TYPES.md).

**¿Por qué bare en lugar de compose?**

La razón principal es la red. Los servicios de compose que necesitan llegar al servidor de desarrollo Vite (para SSR, proxy de assets o conexiones WebSocket de HMR) pueden usar `host.docker.internal` para llegar a los servicios bare en el host DinD. Esto evita configuraciones complejas de red de Docker y coincide con cómo la mayoría de configuraciones de monorepo ajustan `VITE_RUBY_HOST` o variables de entorno similares.

Los servicios bare también obtienen acceso directo al sistema de archivos bind-mounted `/workspace` sin pasar por el overlay de un contenedor interno. Esto hace que el file watcher de Vite responda más rápido a los cambios.

**`install` y `cache`:** El campo `install` se ejecuta antes de que el servicio inicie y nuevamente en cada `coast assign`. Aquí ejecuta `yarn install` para recoger cambios de dependencias al cambiar de rama. El campo `cache` le indica a Coast que preserve `node_modules` a través de cambios de worktree para que las ejecuciones de instalación sean incrementales en lugar de desde cero.

**Solo un `install`:** Observa que `vite-api` no tiene campo `install`. En un monorepo con yarn workspaces, un único `yarn install` en la raíz instala dependencias para todos los workspaces. Ponerlo solo en un servicio evita ejecutarlo dos veces.

## Puertos y Healthchecks

```toml
[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"
```

Cada puerto que quieras que Coast gestione va en `[ports]`. Cada instancia obtiene un [puerto dinámico](../concepts_and_terminology/PORTS.md) (rango alto, siempre accesible) para cada puerto declarado. La instancia [checked-out](../concepts_and_terminology/CHECKOUT.md) también obtiene el puerto canónico (el número que declaraste) reenviado al host.

La sección `[healthcheck]` le dice a Coast cómo sondear la salud de cada puerto. Para puertos con una ruta de healthcheck configurada, Coast envía un HTTP GET cada 5 segundos — cualquier respuesta HTTP cuenta como saludable. Los puertos sin una ruta de healthcheck vuelven a una comprobación de conexión TCP (¿puede el puerto aceptar una conexión?).

En este ejemplo, los servidores web Rails obtienen healthchecks HTTP en `/` porque sirven páginas HTML. Los servidores de desarrollo Vite se dejan sin rutas de healthcheck — no sirven una página raíz significativa, y una comprobación TCP es suficiente para saber que aceptan conexiones.

El estado del healthcheck es visible en la UI de [Coastguard](../concepts_and_terminology/COASTGUARD.md) y mediante `coast ports`.

## Volúmenes

```toml
[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"
```

Todos los volúmenes aquí usan `strategy = "shared"`, lo que significa que un único volumen de Docker se comparte entre todas las instancias de Coast. Esta es la elección correcta para **cachés y artefactos de build** — cosas donde las escrituras concurrentes son seguras y duplicarlas por instancia desperdiciaría espacio en disco y ralentizaría el arranque:

- **`bundle`** — la caché de gems de Ruby. Las gems son las mismas entre ramas. Compartir evita volver a descargar todo el bundle para cada instancia de Coast.
- **`*_rails_cache`** — cachés basadas en archivos de Rails. Aceleran el desarrollo pero no son valiosas — cualquier instancia puede regenerarlas.
- **`*_assets`** — assets compilados. Misma razón que las cachés.

**¿Por qué no compartir para bases de datos?** Coast imprime una advertencia si usas `strategy = "shared"` en un volumen adjunto a un servicio tipo base de datos. Múltiples procesos Postgres escribiendo en el mismo directorio de datos causan corrupción. Para bases de datos, usa [servicios compartidos](../coastfiles/SHARED_SERVICES.md) (un Postgres en el host, como hace esta receta) o `strategy = "isolated"` (cada Coast obtiene su propio volumen). Consulta la página de [Topología de Volúmenes](../concepts_and_terminology/VOLUMES.md) para la matriz completa de decisión.

## Estrategias de Assign

```toml
[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

La sección `[assign]` controla qué ocurre con cada servicio cuando ejecutas `coast assign` para cambiar una instancia de Coast a un worktree diferente. Hacer esto bien es la diferencia entre un cambio de rama de 5 segundos y uno de 60 segundos.

### `default = "none"`

Establecer el valor por defecto en `"none"` significa que cualquier servicio no listado explícitamente en `[assign.services]` se deja intacto al cambiar de rama. Esto es crítico para bases de datos y cachés — Postgres, Redis y los servicios de infraestructura no cambian entre ramas y reiniciarlos es trabajo desperdiciado.

### Estrategias por servicio

| Servicio | Estrategia | Por qué |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | Estos ejecutan servidores de desarrollo con file watchers. El [remount del sistema de archivos](../concepts_and_terminology/FILESYSTEM.md) intercambia el código bajo `/workspace` y el watcher detecta cambios automáticamente. No se necesita reinicio del contenedor. |
| `web-sidekiq`, `api-sidekiq` | `restart` | Los workers en segundo plano cargan el código al arrancar y no observan cambios de archivos. Necesitan reiniciar el contenedor para recoger el código de la nueva rama. |

Solo lista servicios que realmente estén en ejecución. Si tu `COMPOSE_PROFILES` solo inicia un subconjunto de servicios, no listes los inactivos — Coast evalúa la estrategia de assign para cada servicio listado, y reiniciar un servicio que no está ejecutándose es trabajo desperdiciado. Consulta [Optimizaciones de Rendimiento](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md) para más información.

### `exclude_paths`

Esta es la optimización más impactante para monorepos grandes. Le dice a Coast que omita árboles de directorios completos durante la sincronización de archivos ignorados por git (rsync) y el diff de `git ls-files` que se ejecutan en cada assign.

El objetivo es excluir todo lo que tus servicios de Coast no necesitan. En un monorepo con 30.000 archivos, los directorios listados arriba podrían representar 8.000+ archivos irrelevantes para los servicios en ejecución. Excluirlos reduce esa cantidad de stats de archivos en cada cambio de rama.

Para encontrar qué excluir, perfila tu repo:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

Mantén directorios que contengan código fuente montado en servicios en ejecución o librerías compartidas importadas por esos servicios. Excluye todo lo demás — documentación, configs de CI, tooling, apps de otros equipos, clientes móviles, herramientas CLI y cachés vendidas como `.yarn`.

### `rebuild_triggers`

Sin triggers, un servicio con `strategy = "rebuild"` reconstruye su imagen Docker en cada cambio de rama — incluso si no cambió nada que afecte a la imagen. La sección `[assign.rebuild_triggers]` condiciona la reconstrucción a archivos específicos.

En esta receta, los servicios Rails normalmente usan `"hot"` (sin reinicio en absoluto). Pero si alguien cambia el Dockerfile o el Gemfile, los `rebuild_triggers` se activan y fuerzan una reconstrucción completa de la imagen. Si ninguno de los archivos disparadores cambió, Coast omite la reconstrucción por completo. Esto evita builds de imágenes costosos en cambios rutinarios de código mientras sigue capturando cambios a nivel de infraestructura.

## Secrets e Inject

```toml
[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]
```

La sección `[secrets]` extrae valores en tiempo de build y los inyecta en instancias de Coast como variables de entorno.

- **`compose_profiles`** controla qué perfiles de Docker Compose se inician. Así es como limitas un Coast a ejecutar solo los perfiles `api` y `web` en lugar de cada servicio definido en el archivo compose. Sobrescríbelo en tu host con `export COMPOSE_PROFILES=api,web,portal` antes de construir para cambiar qué servicios se inician.
- **`uid` / `gid`** pasan el UID y GID del usuario del host al contenedor, lo cual es común en configuraciones Docker que necesitan que la propiedad de archivos coincida entre host y contenedor.

La sección `[inject]` es más simple — reenvía variables de entorno existentes del host al contenedor de Coast en tiempo de ejecución. Credenciales sensibles como tokens del servidor de gems (`BUNDLE_GEMS__CONTRIBSYS__COM`) permanecen en tu host y se reenvían sin escribirse en ningún archivo de configuración.

Para la referencia completa sobre extractores de secrets y destinos de inyección, consulta [Secrets](../coastfiles/SECRETS.md).

## Adaptar Esta Receta

**Stack de lenguaje diferente:** Reemplaza los volúmenes específicos de Rails (bundle, rails cache, assets) con equivalentes para tu stack — caché de módulos de Go (`/go/pkg/mod`), caché de npm, caché de pip, etc. La estrategia sigue siendo `"shared"` para cualquier caché que sea segura de compartir entre instancias.

**Menos apps:** Si tu monorepo solo tiene una app, elimina las entradas de volumen adicionales y simplifica `[assign.services]` para listar solo tus servicios. Los patrones de servicios compartidos y servicios bare siguen aplicando.

**Bases de datos por instancia:** Si necesitas aislamiento de datos entre instancias de Coast, reemplaza `[shared_services.db]` por un Postgres interno de compose y añade una entrada en `[volumes]` con `strategy = "isolated"`. Cada instancia obtiene su propio volumen de base de datos. Puedes sembrarlo desde tu volumen del host usando `snapshot_source` — consulta la [referencia de Coastfile para Volúmenes](../coastfiles/VOLUMES.md).

**Sin servicios bare:** Si tu frontend está completamente containerizado y no necesita ser accesible vía `host.docker.internal`, elimina las secciones `[services.*]` y `[coast.setup]`. Todo se ejecuta mediante compose.

# Servicios Bare

Si puedes contenerizar tu proyecto, deberías hacerlo. Los servicios bare existen para proyectos que aún no se han contenerizado y en los que añadir un `Dockerfile` y `docker-compose.yml` no es práctico a corto plazo. Son un peldaño, no un destino.

En lugar de que un `docker-compose.yml` orqueste servicios contenerizados, los servicios bare te permiten definir comandos de shell en tu Coastfile y Coast los ejecuta como procesos normales con un supervisor ligero dentro del contenedor de Coast.

## Por qué Contenerizar en su Lugar

Los servicios de [Docker Compose](RUNTIMES_AND_SERVICES.md) te ofrecen:

- Builds reproducibles mediante Dockerfiles
- Health checks que Coast puede esperar durante el arranque
- Aislamiento de procesos entre servicios
- Gestión de volúmenes y redes manejada por Docker
- Una definición portable que funciona en CI, staging y producción

Los servicios bare no te dan nada de eso. Tus procesos comparten el mismo sistema de archivos, la recuperación ante fallos es un bucle de shell, y "funciona en mi máquina" es igual de probable dentro de Coast que fuera. Si tu proyecto ya tiene un `docker-compose.yml`, úsalo.

## Cuándo Tienen Sentido los Servicios Bare

- Estás adoptando Coast para un proyecto que nunca se ha contenerizado y quieres empezar a obtener valor del aislamiento de worktrees y la gestión de puertos de inmediato
- Tu proyecto es una herramienta de un solo proceso o un CLI donde un Dockerfile sería excesivo
- Quieres iterar gradualmente en la contenerización — empieza con servicios bare y pasa a compose más adelante

## Configuración

Los servicios bare se definen con secciones `[services.<name>]` en tu Coastfile. Un Coastfile **no puede** definir tanto `compose` como `[services]` — son mutuamente excluyentes.

```toml
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

Cada servicio tiene cuatro campos:

| Campo | Requerido | Descripción |
|---|---|---|
| `command` | sí | El comando de shell a ejecutar (p. ej., `"npm run dev"`) |
| `port` | no | El puerto en el que el servicio escucha, usado para el mapeo de puertos |
| `restart` | no | Política de reinicio: `"no"` (predeterminado), `"on-failure"`, o `"always"` |
| `install` | no | Uno o más comandos a ejecutar antes de iniciar (p. ej., `"npm install"` o `["npm install", "npm run build"]`) |

### Paquetes de Setup

Dado que los servicios bare se ejecutan como procesos normales, el contenedor de Coast necesita tener instalados los runtimes correctos. Usa `[coast.setup]` para declarar paquetes del sistema:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

Estos se instalan antes de que se inicie cualquier servicio. Sin esto, tus comandos `npm` o `node` fallarán dentro del contenedor.

### Comandos de Install

El campo `install` se ejecuta antes de que el servicio arranque y de nuevo en cada [`coast assign`](ASSIGN.md) (cambio de rama). Aquí es donde va la instalación de dependencias:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

Los comandos de instalación se ejecutan secuencialmente. Si algún comando de instalación falla, el servicio no se inicia.

### Políticas de Reinicio

- **`no`** — el servicio se ejecuta una vez. Si termina, se queda muerto. Úsalo para tareas de una sola ejecución o servicios que quieras gestionar manualmente.
- **`on-failure`** — reinicia el servicio si termina con un código distinto de cero. Las salidas exitosas (código 0) se dejan tal cual. Usa backoff exponencial desde 1 segundo hasta 30 segundos, y se rinde tras 10 fallos consecutivos.
- **`always`** — reinicia ante cualquier salida, incluso si fue exitosa. Mismo backoff que `on-failure`. Úsalo para servidores de larga duración que nunca deberían detenerse.

Si un servicio se ejecuta durante más de 30 segundos antes de fallar, el contador de reintentos y el backoff se reinician — la suposición es que estuvo sano durante un tiempo y el fallo es un problema nuevo.

## Cómo Funciona por Debajo

```text
┌─── Coast: dev-1 ──────────────────────────────────────┐
│                                                       │
│   /coast-supervisor/                                  │
│   ├── web.sh          (runs command, tracks PID)      │
│   ├── worker.sh                                       │
│   ├── start-all.sh    (launches all services)         │
│   ├── stop-all.sh     (SIGTERM via PID files)         │
│   └── ps.sh           (checks PID liveness)           │
│                                                       │
│   /var/log/coast-services/                            │
│   ├── web.log                                         │
│   └── worker.log                                      │
│                                                       │
│   No inner Docker daemon images are used.             │
│   Processes run directly on the container OS.         │
└───────────────────────────────────────────────────────┘
```

Coast genera envoltorios (wrappers) en forma de scripts de shell para cada servicio y los coloca en `/coast-supervisor/` dentro del contenedor DinD. Cada envoltorio registra su PID, redirige la salida a un archivo de log e implementa la política de reinicio como un bucle de shell. No hay Docker Compose, no hay imágenes Docker internas y no hay aislamiento a nivel de contenedor entre servicios.

`coast ps` comprueba si el PID sigue vivo en lugar de consultar a Docker, y `coast logs` hace tail de los archivos de log en lugar de llamar a `docker compose logs`. El formato de salida de logs coincide con el formato de compose `service | line` para que la UI de Coastguard funcione sin cambios.

## Puertos

La configuración de puertos funciona exactamente igual que con Coasts basados en compose. Define los puertos en los que escuchan tus servicios en `[ports]`:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

Los [puertos dinámicos](PORTS.md) se asignan en `coast run`, y [`coast checkout`](CHECKOUT.md) intercambia los puertos canónicos como de costumbre. La única diferencia es que no hay una red Docker entre servicios — todos hacen bind directamente al loopback del contenedor o a `0.0.0.0`.

## Cambio de Rama

Cuando ejecutas `coast assign` en un Coast con servicios bare, sucede lo siguiente:

1. Todos los servicios en ejecución se detienen mediante SIGTERM
2. El worktree cambia a la nueva rama
3. Se vuelven a ejecutar los comandos de instalación (p. ej., `npm install` recoge las dependencias de la nueva rama)
4. Se reinician todos los servicios

Esto equivale a lo que ocurre con compose — `docker compose down`, cambio de rama, rebuild, `docker compose up` — pero con procesos de shell en lugar de contenedores.

## Limitaciones

- **Sin health checks.** Coast no puede esperar a que un servicio bare esté "healthy" como puede hacerlo con un servicio de compose que define un health check. Inicia el proceso y espera lo mejor.
- **Sin aislamiento entre servicios.** Todos los procesos comparten el mismo sistema de archivos y el mismo espacio de nombres de procesos dentro del contenedor de Coast. Un servicio que se comporte mal puede afectar a otros.
- **Sin caché de builds.** Los builds de Docker Compose se cachean capa por capa. Los comandos `install` de servicios bare se ejecutan desde cero en cada assign.
- **La recuperación ante fallos es básica.** La política de reinicio usa un bucle de shell con backoff exponencial. No es un supervisor de procesos como systemd o supervisord.
- **Sin `[omit]` o `[unset]` para servicios.** La composición por tipos de Coastfile funciona con servicios de compose, pero los servicios bare no permiten omitir servicios individuales mediante Coastfiles tipados.

## Migrando a Compose

Cuando estés listo para contenerizar, la ruta de migración es sencilla:

1. Escribe un `Dockerfile` para cada servicio
2. Crea un `docker-compose.yml` que los referencie
3. Sustituye las secciones `[services.*]` en tu Coastfile por un campo `compose` que apunte a tu archivo compose
4. Elimina los paquetes de `[coast.setup]` que ahora gestionan tus Dockerfiles
5. Reconstruye con [`coast build`](BUILDS.md)

Tus mapeos de puertos, [volúmenes](VOLUMES.md), [servicios compartidos](SHARED_SERVICES.md) y configuración de [secrets](SECRETS.md) se mantienen sin cambios. Lo único que cambia es cómo se ejecutan los propios servicios.

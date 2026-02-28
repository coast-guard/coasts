# Registros

Los servicios dentro de un Coast se ejecutan en contenedores anidados — tus servicios de compose son gestionados por un daemon de Docker interno dentro de un contenedor DinD. Esto significa que las herramientas de registro a nivel de host no pueden verlos. Si tu flujo de trabajo incluye un MCP de registros que lee los logs de Docker en el host, solo verá el contenedor DinD externo, no el servidor web, la base de datos o el worker que se ejecutan dentro de él.

La solución es `coast logs`. Cualquier agente o herramienta que necesite leer la salida de servicios desde una instancia de Coast debe usar la CLI de Coast en lugar del acceso a logs de Docker a nivel de host.

## La contrapartida del MCP

Si estás usando un agente de IA con un MCP de registros (una herramienta que captura logs de contenedores Docker desde tu host — ver [MCP Servers](MCP_SERVERS.md)), ese MCP no funcionará para servicios que se ejecutan dentro de un Coast. El daemon de Docker del host ve un contenedor por instancia de Coast — el contenedor DinD — y sus logs son solo la salida de arranque del daemon de Docker interno.

Para capturar los logs reales de los servicios, indica a tu agente que use:

```bash
coast logs <instance> --service <service> --tail <lines>
```

Por ejemplo, si tu agente necesita inspeccionar por qué está fallando un servicio backend:

```bash
coast logs dev-1 --service backend --tail 100
```

Esto es el equivalente de `docker compose logs` pero enrutado a través del daemon de Coast hacia el contenedor DinD interno. Si tienes reglas de agente o prompts del sistema que hagan referencia a un MCP de registros, tendrás que añadir una instrucción que anule este comportamiento cuando trabajes dentro de un Coast.

## `coast logs`

La CLI proporciona varias formas de leer registros de una instancia de Coast:

```bash
coast logs dev-1                           # last 200 lines, all services
coast logs dev-1 --service web             # last 200 lines, web only
coast logs dev-1 --tail 50                 # last 50 lines, then follow
coast logs dev-1 --tail                    # all lines, then follow
coast logs dev-1 --service backend -f      # follow mode (stream new entries)
coast logs dev-1 --service web --tail 100  # last 100 lines + follow
```

Sin `--tail` o `-f`, el comando devuelve las últimas 200 líneas y sale. Con `--tail`, transmite la cantidad solicitada de líneas y luego continúa siguiendo la nueva salida en tiempo real. `-f` / `--follow` habilita el modo de seguimiento por sí solo.

La salida usa el formato de logs de compose con un prefijo de servicio en cada línea:

```text
web       | 2026/02/28 01:49:34 Listening on :3000
backend   | 2026/02/28 01:49:34 [INFO] Server started on :8080
backend   | 2026/02/28 01:49:34 [ProcessCreditsJob] starting at 2026-02-28T01:49:34Z
redis     | 1:M 28 Feb 2026 01:49:30.123 * Ready to accept connections
```

También puedes filtrar por servicio con la sintaxis posicional heredada (`coast logs dev-1 web`), pero se prefiere el flag `--service`.

## Pestaña de Registros de Coastguard

La interfaz web de Coastguard ofrece una experiencia de visualización de logs más rica con transmisión en tiempo real mediante WebSocket.

![Logs tab in Coastguard](../../assets/coastguard-logs.png)
*La pestaña Logs de Coastguard transmitiendo la salida del servicio backend con filtrado por servicio y búsqueda.*

La pestaña Logs ofrece:

- **Transmisión en tiempo real** — los logs llegan a través de una conexión WebSocket a medida que se producen, con un indicador de estado que muestra el estado de la conexión.
- **Filtro de servicio** — un desplegable poblado a partir de los prefijos de servicio del flujo de logs. Selecciona un único servicio para enfocarte en su salida.
- **Búsqueda** — filtra las líneas mostradas por texto o regex (activa el botón de asterisco para el modo regex). Los términos coincidentes se resaltan.
- **Recuentos de líneas** — muestra líneas filtradas vs líneas totales (p. ej., "200 / 971 lines").
- **Limpiar** — trunca los archivos de log del contenedor interno y reinicia el visor.
- **Pantalla completa** — expande el visor de logs para llenar la pantalla.

Las líneas de log se renderizan con soporte de color ANSI, resaltado por nivel de log (ERROR en rojo, WARN en ámbar, INFO en azul, DEBUG en gris), atenuación de marcas de tiempo y distintivos de servicio coloreados para diferenciar visualmente entre servicios.

Los servicios compartidos que se ejecutan en el daemon del host tienen su propio visor de logs accesible desde la pestaña Shared Services. Consulta [Shared Services](SHARED_SERVICES.md) para más detalles.

## Cómo funciona

Cuando ejecutas `coast logs`, el daemon ejecuta `docker compose logs` dentro del contenedor DinD vía `docker exec` y transmite la salida de vuelta a tu terminal (o a la UI de Coastguard vía WebSocket).

```text
coast logs dev-1 --service web --tail 50
  │
  ├── CLI sends LogsRequest to daemon (Unix socket)
  │
  ├── Daemon resolves instance → container ID
  │
  ├── Daemon exec's into DinD container:
  │     docker compose logs --tail 50 --follow web
  │
  └── Output streams back chunk by chunk
        └── CLI prints to stdout / Coastguard renders in UI
```

Para [bare services](BARE_SERVICES.md), el daemon sigue (tail) los archivos de log en `/var/log/coast-services/` en lugar de llamar a `docker compose logs`. El formato de salida es el mismo (`service  | line`), por lo que el filtrado por servicio funciona de forma idéntica en ambos casos.

## Comandos relacionados

- `coast ps <instance>` — comprueba qué servicios se están ejecutando y su estado. Consulta [Runtimes and Services](RUNTIMES_AND_SERVICES.md).
- [`coast exec <instance>`](EXEC_AND_DOCKER.md) — abre una shell dentro del contenedor de Coast para depuración manual.

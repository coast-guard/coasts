# Exec y Docker

`coast exec` te deja en una shell dentro del contenedor DinD de Coast. Tu directorio de trabajo es `/workspace` — la [raíz del proyecto montada por bind](FILESYSTEM.md) donde vive tu Coastfile. Esta es la forma principal de ejecutar comandos, inspeccionar archivos o depurar servicios dentro de un Coast desde tu máquina host.

`coast docker` es el comando complementario para hablar directamente con el daemon de Docker interno.

## `coast exec`

Abre una shell dentro de una instancia de Coast:

```bash
coast exec dev-1
```

Esto inicia una sesión `sh` en `/workspace`. Los contenedores de Coast están basados en Alpine, por lo que la shell predeterminada es `sh`, no `bash`.

También puedes ejecutar un comando específico sin entrar en una shell interactiva:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
```

Todo lo que va después del nombre de la instancia se pasa como el comando. Usa `--` para separar las flags que pertenecen a tu comando de las flags que pertenecen a `coast exec`.

### Directorio de trabajo

La shell inicia en `/workspace`, que es la raíz de tu proyecto en el host montada por bind dentro del contenedor. Esto significa que tu código fuente, Coastfile y todos los archivos del proyecto están ahí mismo:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

Cualquier cambio que hagas en archivos bajo `/workspace` se refleja en el host inmediatamente — es un montaje por bind, no una copia.

### Interactivo vs No interactivo

Cuando stdin es un TTY (estás escribiendo en un terminal), `coast exec` evita el daemon por completo y ejecuta `docker exec -it` directamente para un passthrough completo del TTY. Esto significa que los colores, el movimiento del cursor, el autocompletado con tab y los programas interactivos funcionan como se espera.

Cuando stdin está canalizado o en scripts (CI, flujos de trabajo de agentes, `coast exec dev-1 -- some-command | grep foo`), la solicitud pasa por el daemon y devuelve stdout, stderr estructurados y un código de salida.

### Permisos de archivos

El exec se ejecuta como el UID:GID de tu usuario del host, por lo que los archivos creados dentro del Coast tienen la propiedad correcta en el host. Sin desajustes de permisos entre host y contenedor.

## `coast docker`

Mientras que `coast exec` te da una shell en el propio contenedor DinD, `coast docker` te permite ejecutar comandos de la CLI de Docker contra el daemon de Docker **interno** — el que gestiona tus servicios de compose.

```bash
coast docker dev-1                    # por defecto: docker ps
coast docker dev-1 ps                 # igual que arriba
coast docker dev-1 compose ps         # docker compose ps (servicios internos)
coast docker dev-1 images             # lista imágenes en el daemon interno
coast docker dev-1 compose logs web   # docker compose logs para un servicio
```

Cada comando que pases se prefija con `docker` automáticamente. Así, `coast docker dev-1 compose ps` ejecuta `docker compose ps` dentro del contenedor de Coast, hablando con el daemon interno.

### `coast exec` vs `coast docker`

La diferencia es a qué estás apuntando:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` en el contenedor DinD | El propio contenedor de Coast (tus archivos del proyecto, herramientas instaladas) |
| `coast docker dev-1 ps` | `docker ps` en el contenedor DinD | El daemon de Docker interno (tus contenedores de servicios de compose) |
| `coast docker dev-1 compose logs web` | `docker compose logs web` en el contenedor DinD | Los logs de un servicio específico de compose a través del daemon interno |

Usa `coast exec` para trabajo a nivel de proyecto — ejecutar tests, instalar dependencias, inspeccionar archivos. Usa `coast docker` cuando necesites ver qué está haciendo el daemon de Docker interno — estado de contenedores, imágenes, redes, operaciones de compose.

## Pestaña Exec de Coastguard

La UI web de Coastguard proporciona un terminal interactivo persistente conectado por WebSocket.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*La pestaña Exec de Coastguard mostrando una sesión de shell en /workspace dentro de una instancia de Coast.*

El terminal está impulsado por xterm.js y ofrece:

- **Sesiones persistentes** — las sesiones de terminal sobreviven a la navegación por la página y a los refrescos del navegador. Al reconectar se reproduce el buffer de scrollback para que continúes donde lo dejaste.
- **Múltiples pestañas** — abre varias shells a la vez. Cada pestaña es una sesión independiente.
- Pestañas de **[shell de agente](AGENT_SHELLS.md)** — genera shells dedicadas para agentes de IA de programación, con seguimiento de estado activo/inactivo.
- **Modo pantalla completa** — expande el terminal para llenar la pantalla (Escape para salir).

Más allá de la pestaña de exec a nivel de instancia, Coastguard también proporciona acceso a terminal en otros niveles:

- **Exec de servicio** — haz clic en un servicio individual desde la pestaña Services para obtener una shell dentro de ese contenedor interno específico (esto hace un doble `docker exec` — primero dentro del contenedor DinD, y luego dentro del contenedor del servicio).
- **Exec de [servicio compartido](SHARED_SERVICES.md)** — obtén una shell dentro de un contenedor de servicio compartido a nivel de host.
- **Terminal del host** — una shell en tu máquina host en la raíz del proyecto, sin entrar en un Coast en absoluto.

## Cuándo usar cada uno

- **`coast exec`** — ejecuta comandos a nivel de proyecto (npm install, go test, inspección de archivos, depuración) dentro del contenedor DinD.
- **`coast docker`** — inspecciona o administra el daemon de Docker interno (estado de contenedores, imágenes, redes, operaciones de compose).
- **Pestaña Exec de Coastguard** — depuración interactiva con sesiones persistentes, múltiples pestañas y soporte de shell de agentes. Ideal cuando quieres mantener varios terminales abiertos mientras navegas el resto de la UI.
- **`coast logs`** — para leer la salida de los servicios, usa `coast logs` en lugar de `coast docker compose logs`. Ver [Logs](LOGS.md).
- **`coast ps`** — para comprobar el estado de los servicios, usa `coast ps` en lugar de `coast docker compose ps`. Ver [Runtimes and Services](RUNTIMES_AND_SERVICES.md).

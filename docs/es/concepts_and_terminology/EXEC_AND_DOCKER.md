# Exec y Docker

`coast exec` te introduce en un shell dentro del contenedor DinD de Coast. Tu directorio de trabajo es `/workspace` — la [raíz del proyecto montada con bind](FILESYSTEM.md) donde vive tu Coastfile. Esta es la forma principal de ejecutar comandos, inspeccionar archivos o depurar servicios dentro de un Coast desde tu máquina host.

`coast docker` es el comando complementario para hablar directamente con el daemon interno de Docker.

## `coast exec`

Abre un shell dentro de una instancia de Coast:

```bash
coast exec dev-1
```

Esto inicia una sesión `sh` en `/workspace`. Los contenedores de Coast están basados en Alpine, por lo que el shell predeterminado es `sh`, no `bash`.

También puedes ejecutar un comando específico sin entrar en un shell interactivo:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
coast exec dev-1 --service web
coast exec dev-1 --service web -- php artisan test
```

Todo lo que viene después del nombre de la instancia se pasa como comando. Usa `--` para separar las flags que pertenecen a tu comando de las flags que pertenecen a `coast exec`.

Pasa `--service <name>` para apuntar a un contenedor de servicio de compose específico en lugar del contenedor externo de Coast. Pasa `--root` cuando necesites acceso root sin procesar al contenedor en lugar del mapeo UID:GID del host predeterminado de Coast.

### Directorio de trabajo

El shell se inicia en `/workspace`, que es la raíz de tu proyecto host montada con bind dentro del contenedor. Esto significa que tu código fuente, Coastfile y todos los archivos del proyecto están ahí mismo:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

Cualquier cambio que hagas en archivos bajo `/workspace` se refleja en el host inmediatamente — es un bind mount, no una copia.

### Interactivo vs No interactivo

Cuando stdin es un TTY (estás escribiendo en una terminal), `coast exec` omite el daemon por completo y ejecuta `docker exec -it` directamente para un passthrough completo de TTY. Esto significa que los colores, el movimiento del cursor, el autocompletado con tabulador y los programas interactivos funcionan como se espera.

Cuando stdin está canalizado o se ejecuta desde scripts (CI, flujos de trabajo de agentes, `coast exec dev-1 -- some-command | grep foo`), la solicitud pasa por el daemon y devuelve stdout, stderr y un código de salida estructurados.

### Permisos de archivos

El exec se ejecuta con el UID:GID de tu usuario del host, por lo que los archivos creados dentro de Coast tienen la propiedad correcta en el host. No hay desajustes de permisos entre host y contenedor.

## `coast docker`

Mientras que `coast exec` te da un shell en el propio contenedor DinD, `coast docker` te permite ejecutar comandos de Docker CLI contra el daemon **interno** de Docker — el que gestiona tus servicios de compose.

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps for the active Coast-managed stack
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

Cada comando que pases recibe automáticamente el prefijo `docker`. Así que `coast docker dev-1 compose ps` ejecuta `docker compose ps` dentro del contenedor de Coast, hablando con el daemon interno.

### `coast exec` vs `coast docker`

La distinción está en lo que estás apuntando:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` in DinD container | The Coast container itself (your project files, installed tools) |
| `coast exec dev-1 --service web` | `docker exec ... sh` in the resolved inner service container | A specific compose service container |
| `coast docker dev-1 ps` | `docker ps` in DinD container | The inner Docker daemon (your compose service containers) |
| `coast docker dev-1 compose logs web` | `docker compose logs web` in DinD container | A specific compose service's logs via the inner daemon |

Usa `coast exec` para trabajo a nivel de proyecto — ejecutar pruebas, instalar dependencias, inspeccionar archivos. Usa `coast docker` cuando necesites ver qué está haciendo el daemon interno de Docker — estado de contenedores, imágenes, redes, operaciones de compose.

## Pestaña Exec de Coastguard

La UI web de Coastguard proporciona una terminal interactiva persistente conectada por WebSocket.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*La pestaña Exec de Coastguard mostrando una sesión de shell en /workspace dentro de una instancia de Coast.*

La terminal está impulsada por xterm.js y ofrece:

- **Sesiones persistentes** — las sesiones de terminal sobreviven a la navegación por páginas y a las actualizaciones del navegador. Al reconectarte se reproduce el búfer de scrollback para que continúes donde lo dejaste.
- **Múltiples pestañas** — abre varios shells a la vez. Cada pestaña es una sesión independiente.
- **Pestañas de [shell de agente](AGENT_SHELLS.md)** — genera shells de agente dedicados para agentes de codificación de IA, con seguimiento de estado activo/inactivo.
- **Modo de pantalla completa** — expande la terminal para llenar la pantalla (Escape para salir).

Más allá de la pestaña exec a nivel de instancia, Coastguard también proporciona acceso a terminal en otros niveles:

- **Exec de servicio** — haz clic en un servicio individual desde la pestaña Services para obtener un shell dentro de ese contenedor interno específico (esto hace un doble `docker exec` — primero dentro del contenedor DinD, luego dentro del contenedor de servicio).
- **Exec de [servicio compartido](SHARED_SERVICES.md)** — obtén un shell dentro de un contenedor de servicio compartido a nivel host.
- **Terminal del host** — un shell en tu máquina host en la raíz del proyecto, sin entrar en un Coast en absoluto.

## Cuándo usar cada uno

- **`coast exec`** — ejecuta comandos a nivel de proyecto dentro del contenedor DinD, o pasa `--service` para abrir un shell o ejecutar un comando dentro de un contenedor de servicio de compose específico.
- **`coast docker`** — inspecciona o gestiona el daemon interno de Docker (estado de contenedores, imágenes, redes, operaciones de compose).
- **Pestaña Exec de Coastguard** — depuración interactiva con sesiones persistentes, múltiples pestañas y soporte para shells de agente. Es la mejor opción cuando quieres mantener varias terminales abiertas mientras navegas por el resto de la UI.
- **`coast logs`** — para leer la salida de los servicios, usa `coast logs` en lugar de `coast docker compose logs`. Consulta [Logs](LOGS.md).
- **`coast ps`** — para comprobar el estado de los servicios, usa `coast ps` en lugar de `coast docker compose ps`. Consulta [Runtimes and Services](RUNTIMES_AND_SERVICES.md).

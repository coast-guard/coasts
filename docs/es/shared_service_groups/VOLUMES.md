# Volúmenes de SSG

Dentro de `[shared_services.<name>]`, el arreglo `volumes` usa la sintaxis estándar de Docker Compose:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

Una `/` inicial significa una **ruta bind del host** -- los bytes viven en el sistema de archivos del host y el servicio interno los lee y escribe en su lugar. Sin una barra inicial, p. ej. `pg_wal:/var/lib/postgresql/wal`, el origen es un **volumen con nombre de Docker que vive dentro del daemon Docker anidado del SSG** -- sobrevive a `coast ssg rm` y se elimina con `coast ssg rm --with-data`. Se aceptan ambas formas.

Rechazado al analizar: rutas relativas (`./data:/...`), componentes `..`, volúmenes solo de contenedor (sin origen), y destinos duplicados dentro de un mismo servicio.

## Reutilizar un volumen de Docker desde docker-compose o un servicio compartido inline

Si ya tienes datos dentro de un volumen con nombre de Docker del host -- de `docker-compose up`, de un inline `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]`, o de un `docker volume create` hecho a mano -- puedes hacer que el SSG lea los mismos bytes montando por bind el directorio subyacente del host del volumen:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

El lado izquierdo es la ruta del sistema de archivos del host de un volumen Docker existente; `docker volume inspect <name>` la informa como el campo `Mountpoint`. Coast no copia bytes -- el SSG lee y escribe los mismos archivos que docker-compose usó. `coast ssg rm` (sin `--with-data`) deja el volumen intacto, por lo que docker-compose también puede seguir usándolo.

> **¿Por qué no simplemente `infra_postgres_data:/var/lib/postgresql/data`?** Eso funciona para `[shared_services.*]` inline (el volumen se crea en el daemon Docker del host, donde docker-compose puede verlo). *No* funciona de la misma manera dentro de un SSG -- un nombre sin una barra inicial crea un volumen nuevo dentro del daemon Docker anidado del SSG, aislado del host. Usa en su lugar la ruta del mountpoint del volumen cuando quieras compartir datos con cualquier cosa que se ejecute en el daemon del host.

### `coast ssg import-host-volume`

`coast ssg import-host-volume` resuelve el `Mountpoint` del volumen mediante `docker volume inspect` y emite (o aplica) la línea `volumes` equivalente, para que no construyas manualmente la ruta `/var/lib/docker/volumes/<name>/_data`.

El modo snippet (predeterminado) imprime el fragmento TOML para pegar:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

La salida es un bloque `[shared_services.postgres]` con la nueva entrada `volumes = [...]` ya fusionada:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

El modo apply reescribe `Coastfile.shared_service_groups` en su lugar y guarda el original en `Coastfile.shared_service_groups.bak`:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

Indicadores:

- `<VOLUME>` (posicional) -- volumen con nombre de Docker del host. Debe existir ya (`docker volume inspect` es la comprobación); de lo contrario, créalo o renómbralo primero con `docker volume create`.
- `--service` -- la sección `[shared_services.<name>]` que se editará. La sección ya debe existir.
- `--mount` -- ruta absoluta del contenedor. Las rutas relativas se rechazan. Las rutas de montaje duplicadas en el mismo servicio son errores graves.
- `--file` / `--working-dir` / `--config` -- descubrimiento del Coastfile de SSG, mismas reglas que `coast ssg build`.
- `--apply` -- reescribe el Coastfile en su lugar. No puede combinarse con `--config` (el texto inline no tiene nada a lo que volver a escribir).

El archivo `.bak` contiene los bytes originales textualmente, por lo que puedes recuperar el estado exacto previo a apply.

`/var/lib/docker/volumes/<name>/_data` es la ruta que Docker ha usado como mountpoint de volúmenes durante muchos años y es lo que `docker volume inspect` informa hoy. Docker no promete formalmente mantener esta ruta para siempre; si una futura versión de Docker mueve los volúmenes a otro lugar, vuelve a ejecutar `coast ssg import-host-volume` para obtener la nueva ruta.

## Permisos

Varias imágenes se niegan a arrancar cuando su directorio de datos pertenece al usuario incorrecto. Postgres (UID 999 en la etiqueta debian, UID 70 en la etiqueta alpine), MySQL/MariaDB (UID 999), y MongoDB (UID 999) son los infractores más comunes. Si el directorio del host pertenece a root, Postgres sale al arrancar con un escueto "data directory has wrong ownership".

La solución es un comando:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

Ejecuta esto antes de `coast ssg run`. Si el directorio todavía no existe, `coast ssg run` lo crea con la propiedad predeterminada (root en Linux, tu usuario en macOS a través de Docker Desktop). Esa propiedad predeterminada normalmente es incorrecta para Postgres. Si llegaste aquí mediante `coast ssg import-host-volume` y `docker-compose up` ya había hecho `chown` del volumen en el primer arranque, ya estás bien.

## `coast ssg doctor`

`coast ssg doctor` es una comprobación de solo lectura que se ejecuta contra el SSG del proyecto actual (resuelto desde el `Coastfile` del cwd en `[coast].name` o `--working-dir`). Imprime un hallazgo por cada par `(service, host-bind)` en la build activa, además de hallazgos de extracción de secretos (ver [Secrets](SECRETS.md)).

Para cada imagen conocida (Postgres, MySQL, MariaDB, MongoDB) consulta una tabla UID/GID incorporada, compara con `stat(2)` en cada ruta del host, y emite:

- `ok` cuando el propietario coincide con lo que la imagen espera.
- `warn` cuando difiere. El mensaje incluye el comando `chown` para corregirlo.
- `info` cuando el directorio todavía no existe, o cuando la imagen coincidente solo tiene volúmenes con nombre (nada que comprobar del lado del host).

Los servicios cuyas imágenes no están en la tabla de imágenes conocidas se omiten silenciosamente. Forks como `ghcr.io/baosystems/postgis` no se señalan -- el doctor prefiere no decir nada antes que emitir una advertencia incorrecta.

```bash
coast ssg doctor
```

Salida de ejemplo con un directorio de Postgres que no coincide:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor no modifica nada. Los permisos sobre bytes que pones en tu sistema de archivos del host no son algo que Coast muta silenciosamente.

## Notas de plataforma

- **macOS Docker Desktop.** Las rutas raw del host deben estar listadas en Settings -> Resources -> File Sharing. Los valores predeterminados incluyen `/Users`, `/Volumes`, `/private`, `/tmp`. `/var/coast-data` **no** está en la lista predeterminada en macOS -- prefiere `$HOME/coast-data/...` para rutas nuevas, o añade `/var/coast-data` a File Sharing. La forma `/var/lib/docker/volumes/<name>/_data` *no* es una ruta del host -- Docker la resuelve dentro de su propia VM -- así que funciona sin una entrada en File Sharing.
- **WSL2.** Prefiere rutas nativas de WSL (`~`, `/mnt/wsl/...`). `/mnt/c/...` funciona pero es lento debido al protocolo 9P que hace de puente con el sistema de archivos del host Windows.
- **Linux.** Sin sorpresas.

## Ciclo de vida

- `coast ssg rm` -- elimina el contenedor DinD externo del SSG. **El contenido del volumen no se toca**, el contenido de los bind mounts del host no se toca, el keystore no se toca. Cualquier otra cosa que use el mismo volumen Docker sigue funcionando.
- `coast ssg rm --with-data` -- elimina los volúmenes que viven **dentro del daemon Docker anidado del SSG** (la forma `name:path` sin una barra inicial). Los bind mounts del host y los volúmenes Docker externos siguen intactos -- Coast no es su dueño.
- `coast ssg build` -- nunca toca los volúmenes. Solo escribe un manifiesto y (cuando se declara `[secrets]`) filas del keystore.
- `coast ssg run` / `start` / `restart` -- crea directorios de bind mount del host si no existen (con la propiedad predeterminada -- ver [Permisos](#permisos)).

## Ver también

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- esquema TOML completo, incluida la sintaxis de volúmenes
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- estrategias de volúmenes compartidos, aislados y inicializados por snapshot para servicios que no son SSG
- [Building](BUILDING.md) -- de dónde viene el manifiesto
- [Lifecycle](LIFECYCLE.md) -- cuándo se crean, se detienen y se eliminan los volúmenes
- [Secrets](SECRETS.md) -- los secretos inyectados por archivo terminan en `~/.coast/ssg/runs/<project>/secrets/<basename>` y se montan por bind en los servicios internos como solo lectura

# Coastfile.shared_service_groups

`Coastfile.shared_service_groups` es un Coastfile tipado que declara los servicios que ejecutará el Grupo de Servicios Compartidos (SSG) de tu proyecto. Se ubica junto a un `Coastfile` normal, y el nombre del proyecto proviene de `[coast].name` en ese archivo hermano -- no lo repites aquí. Cada proyecto tiene exactamente un archivo de este tipo (en tu worktree); el contenedor `<project>-ssg` ejecuta los servicios que declara. Otros Coastfiles consumidores del mismo proyecto pueden hacer referencia a estos servicios con `[shared_services.<name>] from_group = true`.

Para el concepto, ciclo de vida, volúmenes, secretos y conexión de consumidores, consulta la [documentación de Shared Service Groups](../shared_service_groups/README.md).

## Discovery

`coast ssg build` encuentra el archivo usando las mismas reglas que `coast build`:

- Predeterminado: buscar `Coastfile.shared_service_groups` o `Coastfile.shared_service_groups.toml` en el directorio de trabajo actual. Ambas formas son equivalentes; la variante `.toml` tiene prioridad cuando ambas existen.
- `-f <path>` / `--file <path>` apunta a un archivo arbitrario.
- `--working-dir <dir>` desacopla la raíz del proyecto de la ubicación del Coastfile.
- `--config '<toml>'` acepta TOML en línea para flujos automatizados.

## Accepted Sections

Solo se aceptan `[ssg]`, `[shared_services.<name>]`, `[secrets.<name>]` y `[unset]`. Cualquier otra clave de nivel superior (`[coast]`, `[ports]`, `[services]`, `[volumes]`, `[assign]`, `[omit]`, `[inject]`, ...) se rechaza durante el parseo.

`[ssg] extends = "<path>"` y `[ssg] includes = ["<path>", ...]` son compatibles para composición. Consulta [Inheritance](#inheritance) más abajo.

## `[ssg]`

Configuración SSG de nivel superior.

```toml
[ssg]
runtime = "dind"
```

### `runtime` (optional)

Runtime de contenedor para el DinD SSG externo. `dind` es el único valor compatible hoy; el campo es opcional y su valor predeterminado es `dind`.

## `[shared_services.<name>]`

Un bloque por servicio. La clave TOML (`postgres`, `redis`, ...) se convierte en el nombre del servicio al que hacen referencia los Coastfiles consumidores.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

### `image` (required)

La imagen Docker que se ejecutará dentro del daemon Docker interno del SSG. Se acepta cualquier imagen pública o privada que el host pueda descargar.

### `ports`

Puertos del contenedor en los que escucha el servicio. **Solo enteros simples.**

```toml
ports = [5432]
ports = [5432, 5433]
```

- Un mapeo `"HOST:CONTAINER"` (`"5432:5432"`) se **rechaza**. Las publicaciones de host del SSG siempre son dinámicas -- nunca eliges el puerto del host.
- Se permite un array vacío (o que el campo se omita por completo). Los sidecars sin puertos expuestos no presentan problema.

Cada puerto se convierte en un mapeo `PUBLISHED:CONTAINER` en el DinD externo en el momento de `coast ssg run`, donde `PUBLISHED` es un puerto de host asignado dinámicamente. Se asigna además un puerto virtual separado por proyecto para un enrutamiento estable del consumidor -- consulta [Routing](../shared_service_groups/ROUTING.md).

### `env`

Mapa plano de strings reenviado textualmente al entorno del contenedor de servicio interno.

```toml
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "app" }
```

Los valores de env **no** se capturan en el manifiesto de build. Solo se registran las claves, en línea con la postura de seguridad de `coast build`.

Para valores que no quieres codificar directamente en el Coastfile (contraseñas, tokens de API), usa la sección `[secrets.*]` descrita más abajo -- extrae del host en tiempo de build e inyecta en tiempo de ejecución.

### `volumes`

Array de strings de volumen estilo Docker Compose. Cada entrada es una de las siguientes:

```toml
volumes = [
    "/var/coast-data/postgres:/var/lib/postgresql/data",   # host bind mount
    "pg_wal:/var/lib/postgresql/wal",                       # inner named volume
]
```

**Host bind mount** -- el origen comienza con `/`. Los bytes viven en el sistema de archivos real del host. Tanto el DinD externo como el servicio interno montan por bind **la misma ruta del host como string**. Consulta [Volumes -> Symmetric-Path Plan](../shared_service_groups/VOLUMES.md#the-symmetric-path-plan).

**Inner named volume** -- el origen es un nombre de volumen Docker (sin `/`). El volumen vive dentro del daemon Docker interno del SSG. Persiste entre reinicios del SSG; opaco para el host.

Se rechaza durante el parseo:

- Rutas relativas (`./data:/...`).
- Componentes `..`.
- Volúmenes solo de contenedor (sin origen).
- Targets duplicados dentro de un mismo servicio.

### `auto_create_db`

Cuando es `true`, el daemon crea una base de datos `{instance}_{project}` dentro de este servicio para cada Coast consumidor que se ejecuta. Solo se aplica a imágenes de bases de datos reconocidas (Postgres, MySQL). El valor predeterminado es `false`.

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
auto_create_db = true
```

Un Coastfile consumidor puede sobrescribir este valor por proyecto -- consulta [Consuming -> auto_create_db](../shared_service_groups/CONSUMING.md#auto_create_db).

### `inject` (not allowed)

`inject` **no** es válido en definiciones de servicios SSG. La inyección es una preocupación del lado del consumidor (distintos Coastfiles consumidores pueden querer que el mismo Postgres del SSG se exponga bajo nombres de variables de entorno diferentes). Consulta [Coastfile: Shared Services](SHARED_SERVICES.md#inject) para la semántica de `inject` del lado del consumidor.

## `[secrets.<name>]`

El bloque `[secrets.*]` en `Coastfile.shared_service_groups` extrae credenciales del lado del host en el momento de `coast ssg build` y las inyecta en los servicios internos del SSG en el momento de `coast ssg run`. El esquema refleja el de `[secrets.*]` del Coastfile normal (consulta [Secrets](SECRETS.md) para la referencia de campos); el comportamiento específico de SSG está documentado en [SSG Secrets](../shared_service_groups/SECRETS.md).

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"

[secrets.tls_cert]
extractor = "file"
path = "/Users/me/certs/dev.pem"
inject = "file:/etc/ssl/certs/server.pem"
```

Están disponibles los mismos extractores (`env`, `file`, `command`, `keychain`, `coast-extractor-<name>` personalizado). La directiva `inject` selecciona si el valor llega como variable de entorno o como archivo dentro del contenedor de servicio interno del SSG.

De forma predeterminada, un secreto nativo de SSG se inyecta en **todos** los `[shared_services.*]` declarados. Para apuntar a un subconjunto, enumera los nombres de los servicios explícitamente:

```toml
[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]      # only mounted on the postgres service
```

Los valores secretos extraídos se almacenan cifrados en `~/.coast/keystore.db` bajo `coast_image = "ssg:<project>"` -- un espacio de nombres separado de las entradas normales del keystore de Coast. Consulta [SSG Secrets](../shared_service_groups/SECRETS.md) para el ciclo de vida completo, incluido el verbo `coast ssg secrets clear`.

## Inheritance

Los Coastfiles SSG admiten el mismo mecanismo `extends` / `includes` / `[unset]` que los Coastfiles normales. Consulta [Coastfile Inheritance](INHERITANCE.md) para el modelo mental compartido; esta sección documenta la forma específica de SSG.

### `[ssg] extends` -- incorporar un Coastfile padre

```toml
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
```

El archivo padre se resuelve en relación con el directorio padre del hijo. Se aplica el desempate `.toml` (el parser intenta primero `Coastfile.ssg-base.toml`, luego `Coastfile.ssg-base` simple). También se aceptan rutas absolutas.

### `[ssg] includes` -- fusionar archivos fragmento

```toml
[ssg]
includes = ["dev-seed.toml", "extra-caches.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
```

Los fragmentos se fusionan en orden antes del propio archivo que los incluye. Las rutas de los fragmentos se resuelven en relación con el directorio padre del archivo que los incluye (sin desempate `.toml` -- los fragmentos normalmente se nombran de forma exacta).

**Los fragmentos no pueden usar por sí mismos `extends` ni `includes`.** Deben ser autocontenidos.

### Merge semantics

- **Escalares de `[ssg]`** (`runtime`) -- el hijo gana cuando está presente; de lo contrario, se hereda.
- **`[shared_services.*]`** -- reemplazo por nombre. Si padre e hijo definen ambos `postgres`, la entrada del hijo reemplaza por completo la del padre (reemplazo de entrada completa, no fusión a nivel de campo). Los servicios del padre que el hijo no vuelve a declarar se heredan.
- **`[secrets.*]`** -- reemplazo por nombre, con la misma forma que `[shared_services.*]`. Un secreto hijo con el mismo nombre sobrescribe por completo la configuración del secreto del padre.
- **Orden de carga** -- primero se carga el padre de `extends`, luego cada fragmento de `includes` en orden, y después el propio archivo de nivel superior. Las capas posteriores ganan en caso de colisión.

### `[unset]` -- eliminar servicios o secretos heredados

```toml
[ssg]
extends = "Coastfile.ssg-base"

[unset]
shared_services = ["mongodb"]
secrets = ["pg_password"]
```

Elimina las entradas nombradas **después** de la fusión, de modo que un hijo puede quitar selectivamente algo que proporciona el padre. Se admiten tanto las claves `shared_services` como `secrets`.

Los Coastfiles SSG independientes pueden técnicamente contener `[unset]`, pero se ignora silenciosamente (coincide con el comportamiento del Coastfile normal: unset solo se aplica cuando el archivo participa en herencia).

### Cycles

Los ciclos directos (`A` extiende `B` extiende `A`, o `A` se extiende a sí mismo) producen un error duro con `circular extends/includes dependency detected: '<path>'`. La herencia en diamante (dos rutas separadas que terminan ambas en el mismo padre) está permitida -- el conjunto de visita es por recursión y se desapila al retornar.

### `[omit]` is not applicable

Los Coastfiles normales admiten `[omit]` para quitar servicios / volúmenes del archivo compose. El SSG no tiene un archivo compose del que quitar elementos -- genera el compose interno directamente a partir de las entradas `[shared_services.*]`. Usa `[unset]` para eliminar servicios heredados en su lugar.

### Inline `--config` rejects `extends` / `includes`

`coast ssg build --config '<toml>'` no puede resolver una ruta padre porque no hay una ubicación en disco a la que anclar rutas relativas. Pasar `extends` / `includes` en TOML en línea produce un error duro con `extends and includes require file-based parsing`. Usa `-f <file>` o `--working-dir <dir>` en su lugar.

### Build artifact is the flattened form

`coast ssg build` escribe un TOML independiente en `~/.coast/ssg/<project>/builds/<id>/ssg-coastfile.toml`. El artefacto contiene el resultado fusionado posterior a la herencia sin directivas `extends`, `includes` ni `[unset]`, de modo que el build pueda inspeccionarse o volver a ejecutarse sin que estén presentes los archivos padre / fragmento. El hash `build_id` también refleja la forma aplanada, por lo que un cambio solo en el padre invalida la caché correctamente.

## Example

Postgres + Redis con una contraseña extraída desde env:

```toml
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast" }
auto_create_db = true

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[secrets.pg_password]
extractor = "env"
var = "MY_PG_PASSWORD"
inject = "env:POSTGRES_PASSWORD"
services = ["postgres"]
```

## See Also

- [Shared Service Groups](../shared_service_groups/README.md) -- descripción general del concepto
- [SSG Building](../shared_service_groups/BUILDING.md) -- qué hace `coast ssg build` con este archivo
- [SSG Volumes](../shared_service_groups/VOLUMES.md) -- formas de declaración de volúmenes, permisos y la receta de migración de volúmenes del host
- [SSG Secrets](../shared_service_groups/SECRETS.md) -- la canalización de extracción en tiempo de build / inyección en tiempo de ejecución para `[secrets.*]`
- [SSG Routing](../shared_service_groups/ROUTING.md) -- puertos canónicos / dinámicos / virtuales
- [Coastfile: Shared Services](SHARED_SERVICES.md) -- sintaxis del lado del consumidor `from_group = true`
- [Coastfile: Secrets and Injection](SECRETS.md) -- la referencia normal de `[secrets.*]` de Coastfile
- [Coastfile Inheritance](INHERITANCE.md) -- el modelo mental compartido de `extends` / `includes` / `[unset]`

# Construcción de un Grupo de Servicios Compartidos

`coast ssg build` analiza `Coastfile.shared_service_groups` de tu proyecto, extrae cualquier secreto declarado, descarga cada imagen en la caché de imágenes del host y escribe un artefacto de compilación versionado en `~/.coast/ssg/<project>/builds/<build_id>/`. El comando no es destructivo con respecto a un SSG que ya esté en ejecución: el siguiente `coast ssg run` o `coast ssg start` recogerá la nueva compilación, pero un `<project>-ssg` en ejecución seguirá sirviendo su compilación actual hasta que lo reinicies.

El nombre del proyecto proviene de `[coast].name` en el `Coastfile` hermano. Cada proyecto tiene su propio SSG llamado `<project>-ssg`, su propio directorio de compilación y su propio `latest_build_id`; no existe un "SSG actual" a nivel de host.

Para el esquema TOML completo, consulta [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md).

## Descubrimiento

`coast ssg build` encuentra su Coastfile usando las mismas reglas que `coast build`:

- Sin banderas, busca en el directorio de trabajo actual `Coastfile.shared_service_groups` o `Coastfile.shared_service_groups.toml`. Ambas formas son equivalentes y el sufijo `.toml` tiene prioridad cuando existen ambas.
- `-f <path>` / `--file <path>` apunta a un archivo arbitrario.
- `--working-dir <dir>` desacopla la raíz del proyecto de la ubicación del Coastfile (la misma bandera que `coast build --working-dir`).
- `--config '<inline-toml>'` admite flujos de scripting y CI donde sintetizas el Coastfile en línea.

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

La compilación resuelve el nombre del proyecto a partir del `Coastfile` hermano en el mismo directorio. Si usas `--config` (sin `Coastfile.shared_service_groups` en disco), el cwd aún debe contener un `Coastfile` cuyo `[coast].name` sea el proyecto SSG.

## Qué Hace la Compilación

Cada `coast ssg build` transmite el progreso a través del mismo canal `BuildProgressEvent` que `coast build`, por lo que la CLI renderiza contadores de pasos `[N/M]`.

1. **Analizar** el `Coastfile.shared_service_groups`. `[ssg]`, `[shared_services.*]`, `[secrets.*]` y `[unset]` son las secciones de nivel superior aceptadas. Las entradas de volumen se dividen en montajes bind del host y volúmenes nombrados internos (consulta [Volumes](VOLUMES.md)).
2. **Resolver el build id.** El id tiene la forma `{coastfile_hash}_{YYYYMMDDHHMMSS}`. El hash incorpora la fuente sin procesar, un resumen determinista de los servicios analizados y la configuración `[secrets.*]` (por lo que editar el `extractor` o `var` de un secreto produce un nuevo id).
3. **Sintetizar el `compose.yml` interno.** Cada bloque `[shared_services.*]` se convierte en una entrada en un único archivo Docker Compose. Este es el archivo que el daemon Docker interno del SSG ejecuta mediante `docker compose up -d` en el momento de `coast ssg run`.
4. **Extraer secretos.** Cuando `[secrets.*]` no está vacío, ejecuta cada extractor declarado y almacena el resultado cifrado en `~/.coast/keystore.db` bajo `coast_image = "ssg:<project>"`. Se omite silenciosamente cuando el Coastfile no tiene bloque `[secrets]`. Consulta [Secrets](SECRETS.md) para el pipeline completo.
5. **Descargar y almacenar en caché cada imagen.** Las imágenes se almacenan como tarballs OCI en `~/.coast/image-cache/`, el mismo pool que usa `coast build`. Los aciertos de caché de cualquiera de los dos comandos aceleran al otro.
6. **Escribir el artefacto de compilación** en `~/.coast/ssg/<project>/builds/<build_id>/` con tres archivos: `manifest.json`, `ssg-coastfile.toml` y `compose.yml` (consulta la estructura a continuación).
7. **Actualizar el `latest_build_id` del proyecto.** Esto es una bandera en la base de datos de estado, no un symlink del sistema de archivos. `coast ssg run` y `coast ssg ps` lo leen para saber sobre qué compilación operar.
8. **Poda automática** de compilaciones antiguas hasta conservar las 5 más recientes de este proyecto. Los directorios de artefactos anteriores bajo `~/.coast/ssg/<project>/builds/` se eliminan del disco. Las compilaciones fijadas (consulta "Fijar un proyecto a una compilación específica" más abajo) siempre se conservan.

## Estructura del Artefacto

```text
~/.coast/
  keystore.db                                          (compartido, con espacio de nombres por coast_image)
  keystore.key
  image-cache/                                         (pool compartido de tarballs OCI)
  ssg/
    cg/                                                (proyecto "cg")
      builds/
        b455787d95cfdeb_20260420061903/                (la nueva compilación)
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               (compilación anterior)
          ...
    filemap/                                           (proyecto "filemap" -- árbol separado)
      builds/
        ...
    runs/
      cg/                                              (scratch de ejecución por proyecto)
        compose.override.yml                           (renderizado en coast ssg run)
        secrets/<basename>                             (secretos inyectados como archivo, modo 0600)
```

`manifest.json` captura los metadatos de compilación que le importan al código descendente:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

Los valores de env y las cargas útiles de secretos están intencionalmente ausentes: solo se capturan los nombres de las variables de entorno y los *targets* de inyección. Los valores secretos viven cifrados en el keystore, nunca en los archivos de artefacto.

`ssg-coastfile.toml` es el Coastfile analizado, interpolado y posterior a la validación. Es idéntico byte por byte a lo que el daemon habría visto en el momento del análisis. Útil para auditar una compilación pasada.

`compose.yml` es lo que ejecuta el daemon Docker interno del SSG. Consulta [Volumes](VOLUMES.md) para las reglas de síntesis, especialmente la estrategia de montaje bind de ruta simétrica.

## Inspeccionar una Compilación Sin Ejecutarla

`coast ssg ps` lee directamente `manifest.json` para el `latest_build_id` del proyecto; no inspecciona ningún contenedor. Puedes ejecutarlo inmediatamente después de `coast ssg build` para ver los servicios que se iniciarán en el siguiente `coast ssg run`:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

La columna `PORT` es el puerto interno del contenedor. Los puertos dinámicos del host se asignan en `coast ssg run`; el puerto virtual orientado al consumidor se informa mediante `coast ssg ports`. Consulta [Routing](ROUTING.md) para obtener el panorama completo.

Para examinar cada compilación de un proyecto (con marcas de tiempo, recuentos de servicios y cuál compilación es actualmente la más reciente), usa:

```bash
coast ssg builds-ls
```

## Recompilaciones

Un nuevo `coast ssg build` es la forma canónica de actualizar un SSG. Vuelve a extraer secretos (si los hay), actualiza `latest_build_id` y poda artefactos antiguos. Los consumidores no se recompilan automáticamente: sus referencias `from_group = true` se resuelven en el momento de la compilación del consumidor contra la compilación que fuera actual en ese momento. Para mover un consumidor a un SSG más nuevo, ejecuta `coast build` para el consumidor.

El tiempo de ejecución es tolerante entre recompilaciones: los puertos virtuales permanecen estables por `(project, service, container_port)`, por lo que no hace falta actualizar los consumidores para el enrutamiento. Los cambios de forma (un servicio fue renombrado o eliminado) aparecen como errores de conexión a nivel del consumidor, no como un mensaje de "drift" a nivel de Coast. Consulta [Routing](ROUTING.md) para entender por qué.

## Fijar un proyecto a una compilación específica

Por defecto, el SSG ejecuta el `latest_build_id` del proyecto. Si necesitas congelar un proyecto en una compilación anterior —para reproducir una regresión, comparar A/B dos compilaciones entre worktrees, o mantener una rama de larga duración en una forma conocida y estable— usa los comandos de fijación:

```bash
coast ssg checkout-build <build_id>     # fijar este proyecto a <build_id>
coast ssg show-pin                      # informar la fijación activa (si existe)
coast ssg uncheckout-build              # liberar la fijación; volver a latest
```

Las fijaciones son por proyecto consumidor (una fijación por proyecto, compartida entre worktrees). Cuando está fijado:

- `coast ssg run` inicia automáticamente la compilación fijada en lugar de `latest_build_id`.
- `coast build` valida las referencias `from_group` contra el manifiesto de la compilación fijada.
- `auto_prune` no eliminará el directorio de la compilación fijada, incluso si queda fuera de la ventana de las 5 más recientes.

La SPA de Coastguard muestra una insignia `PINNED` junto al build id cuando hay una fijación activa, y `LATEST` cuando no la hay. Los comandos de fijación también aparecen en [CLI](CLI.md).

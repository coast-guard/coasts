# Builds

Piensa en un build de Coast como una imagen Docker con ayuda extra. Un build es un artefacto basado en directorios que agrupa todo lo necesario para crear instancias de Coast: un [Coastfile](COASTFILE_TYPES.md) resuelto, un archivo compose reescrito, tarballs de imágenes OCI pre-descargadas e inyección de archivos del host. No es una imagen Docker en sí, pero contiene imágenes Docker (como tarballs) además de los metadatos que Coast necesita para conectarlas entre sí.

## What `coast build` Does

Cuando ejecutas `coast build`, el daemon realiza estos pasos en orden:

1. Analiza y valida el Coastfile.
2. Lee el archivo compose y filtra los servicios omitidos.
3. Extrae [secrets](SECRETS.md) de los extractores configurados y los almacena cifrados en el keystore.
4. Construye imágenes Docker para los servicios de compose que tengan directivas `build:` (en el host).
5. Descarga imágenes Docker para los servicios de compose que tengan directivas `image:`.
6. Cachea todas las imágenes como tarballs OCI en `~/.coast/image-cache/`.
7. Si `[coast.setup]` está configurado, construye una imagen base DinD personalizada con los paquetes, comandos y archivos especificados.
8. Escribe el directorio del artefacto del build con el manifiesto, el coastfile resuelto, el compose reescrito y los archivos inyectados.
9. Actualiza el symlink `latest` para que apunte al nuevo build.
10. Auto-purga builds antiguos más allá del límite de retención.

## Where Builds Live

```text
~/.coast/
  images/
    my-project/
      latest -> a3c7d783_20260227143000       (symlink)
      a3c7d783_20260227143000/                (versioned build)
        manifest.json
        coastfile.toml
        compose.yml
        inject/
      b4d8e894_20260226120000/                (older build)
        ...
  image-cache/                                (shared tarball cache)
    postgres_16_a1b2c3d4e5f6.tar
    redis_7_f6e5d4c3b2a1.tar
    coast-built_my-project_web_latest_...tar
```

Cada build obtiene un **ID de build** único con el formato `{coastfile_hash}_{YYYYMMDDHHMMSS}`. El hash incorpora el contenido del Coastfile y la configuración resuelta, por lo que los cambios en el Coastfile producen un nuevo ID de build.

El symlink `latest` siempre apunta al build más reciente para una resolución rápida. Si tu proyecto usa Coastfiles tipados (p. ej., `Coastfile.light`), cada tipo obtiene su propio symlink: `latest-light`.

La caché de imágenes en `~/.coast/image-cache/` se comparte entre todos los proyectos. Si dos proyectos usan la misma imagen de Postgres, el tarball se cachea una sola vez.

## What a Build Contains

Cada directorio de build contiene:

- **`manifest.json`** -- metadatos completos del build: nombre del proyecto, marca de tiempo del build, hash del coastfile, lista de imágenes cacheadas/compiladas, nombres de secrets, servicios omitidos, [estrategias de volúmenes](VOLUMES.md) y más.
- **`coastfile.toml`** -- el Coastfile resuelto (fusionado con el padre si se usa `extends`).
- **`compose.yml`** -- una versión reescrita de tu archivo compose donde las directivas `build:` se reemplazan por etiquetas de imagen precompiladas y se eliminan los servicios omitidos.
- **`inject/`** -- copias de archivos del host de `[inject].files` (p. ej., `~/.gitconfig`, `~/.npmrc`).

## Builds Do Not Contain Secrets

Los secrets se extraen durante el paso de build, pero se almacenan en un keystore cifrado separado en `~/.coast/keystore.db` -- no dentro del directorio del artefacto del build. El manifiesto solo registra los **nombres** de los secrets que se extrajeron, nunca los valores.

Esto significa que los artefactos de build son seguros de inspeccionar sin exponer datos sensibles. Los secrets se descifran y se inyectan más tarde, cuando se crea una instancia de Coast con `coast run`.

## Builds and Docker

Un build implica tres tipos de imágenes Docker:

- **Imágenes construidas** -- los servicios de compose con directivas `build:` se construyen en el host mediante `docker build`, se etiquetan como `coast-built/{project}/{service}:latest` y se guardan como tarballs en la caché de imágenes.
- **Imágenes descargadas** -- los servicios de compose con directivas `image:` se descargan y se guardan como tarballs.
- **Imagen de Coast** -- si `[coast.setup]` está configurado, se construye una imagen Docker personalizada sobre `docker:dind` con los paquetes, comandos y archivos especificados. Se etiqueta como `coast-image/{project}:{build_id}`.

En tiempo de ejecución (`coast run`), estos tarballs se cargan en el daemon [DinD interno](RUNTIMES_AND_SERVICES.md) mediante `docker load`. Esto es lo que hace que las instancias de Coast se inicien rápidamente sin necesidad de descargar imágenes desde un registry.

## Builds and Instances

Cuando ejecutas `coast run`, Coast resuelve el build más reciente (o un `--build-id` específico) y usa sus artefactos para crear la instancia. El ID de build se registra en la instancia.

No necesitas reconstruir para crear más instancias. Un build puede servir a muchas instancias de Coast ejecutándose en paralelo.

## When to Rebuild

Solo reconstruye cuando tu Coastfile, `docker-compose.yml` o la configuración de infraestructura cambien. Reconstruir consume muchos recursos: vuelve a descargar imágenes, vuelve a construir imágenes Docker y vuelve a extraer secrets.

Los cambios de código no requieren un rebuild. Coast monta el directorio de tu proyecto directamente en cada instancia, por lo que las actualizaciones de código se reflejan de inmediato.

## Auto-Pruning

Coast mantiene hasta 5 builds por tipo de Coastfile. Después de cada `coast build` exitoso, los builds más antiguos que excedan el límite se eliminan automáticamente.

Los builds que están siendo usados por instancias en ejecución nunca se purgan, independientemente del límite. Si tienes 7 builds pero 3 de ellos están respaldando instancias activas, los 3 quedan protegidos.

## Manual Removal

Puedes eliminar builds manualmente mediante `coast rm-build` o a través de la pestaña Builds de Coastguard.

- **Eliminación completa del proyecto** (`coast rm-build <project>`) requiere que todas las instancias se detengan y eliminen primero. Elimina todo el directorio de build, las imágenes Docker asociadas, los volúmenes y los contenedores.
- **Eliminación selectiva** (por ID de build, disponible en la UI de Coastguard) omite los builds que están en uso por instancias en ejecución.

## Typed Builds

Si tu proyecto usa múltiples Coastfiles (p. ej., `Coastfile` para la configuración predeterminada y `Coastfile.snap` para volúmenes inicializados desde snapshots), cada tipo mantiene su propio symlink `latest-{type}` y su propio pool de purga de 5 builds.

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

Purgar un build `snap` nunca afecta a los builds `default`, y viceversa.

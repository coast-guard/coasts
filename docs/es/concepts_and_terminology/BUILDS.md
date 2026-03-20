# Builds

Piensa en una build de coast como una imagen de Docker con ayuda adicional. Una build es un artefacto basado en directorio que agrupa todo lo necesario para crear instancias de Coast: un [Coastfile](COASTFILE_TYPES.md) resuelto, un archivo compose reescrito, tarballs de imágenes OCI predescargadas y archivos del host inyectados. No es una imagen de Docker en sí, pero contiene imágenes de Docker (como tarballs) además de los metadatos que Coast necesita para conectarlas entre sí.

## Qué hace `coast build`

Cuando ejecutas `coast build`, el daemon ejecuta estos pasos en orden:

1. Analiza y valida el Coastfile.
2. Lee el archivo compose y filtra los servicios omitidos.
3. Extrae [secretos](SECRETS.md) de los extractores configurados y los almacena cifrados en el keystore.
4. Construye imágenes de Docker para los servicios compose que tienen directivas `build:` (en el host).
5. Descarga imágenes de Docker para los servicios compose que tienen directivas `image:`.
6. Almacena en caché todas las imágenes como tarballs OCI en `~/.coast/image-cache/`.
7. Si `[coast.setup]` está configurado, construye una imagen base DinD personalizada con los paquetes, comandos y archivos especificados.
8. Escribe el directorio del artefacto de build con el manifiesto, el coastfile resuelto, el compose reescrito y los archivos inyectados.
9. Actualiza el enlace simbólico `latest` para que apunte a la nueva build.
10. Elimina automáticamente las builds antiguas que excedan el límite de conservación.

## Dónde viven las builds

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

El enlace simbólico `latest` siempre apunta a la build más reciente para una resolución rápida. Si tu proyecto usa Coastfiles tipados (por ejemplo, `Coastfile.light`), cada tipo obtiene su propio enlace simbólico: `latest-light`.

La caché de imágenes en `~/.coast/image-cache/` se comparte entre todos los proyectos. Si dos proyectos usan la misma imagen de Postgres, el tarball se almacena una sola vez en caché.

## Qué contiene una build

Cada directorio de build contiene:

- **`manifest.json`** -- metadatos completos de la build: nombre del proyecto, marca de tiempo de la build, hash del coastfile, lista de imágenes almacenadas en caché/construidas, nombres de secretos, servicios omitidos, [estrategias de volumen](VOLUMES.md) y más.
- **`coastfile.toml`** -- el Coastfile resuelto (fusionado con el padre si se usa `extends`).
- **`compose.yml`** -- una versión reescrita de tu archivo compose donde las directivas `build:` se reemplazan por etiquetas de imagen preconstruidas, y los servicios omitidos se eliminan.
- **`inject/`** -- copias de archivos del host de `[inject].files` (por ejemplo, `~/.gitconfig`, `~/.npmrc`).

## Las builds no contienen secretos

Los secretos se extraen durante el paso de build, pero se almacenan en un keystore cifrado separado en `~/.coast/keystore.db` -- no dentro del directorio del artefacto de build. El manifiesto solo registra los **nombres** de los secretos que se extrajeron, nunca los valores.

Esto significa que los artefactos de build son seguros de inspeccionar sin exponer datos sensibles. Los secretos se descifran e inyectan después, cuando se crea una instancia de Coast con `coast run`.

## Builds y Docker

Una build involucra tres tipos de imágenes de Docker:

- **Imágenes construidas** -- los servicios compose con directivas `build:` se construyen en el host mediante `docker build`, se etiquetan como `coast-built/{project}/{service}:latest` y se guardan como tarballs en la caché de imágenes.
- **Imágenes descargadas** -- los servicios compose con directivas `image:` se descargan y se guardan como tarballs.
- **Imagen de Coast** -- si `[coast.setup]` está configurado, se construye una imagen de Docker personalizada sobre `docker:dind` con los paquetes, comandos y archivos especificados. Se etiqueta como `coast-image/{project}:{build_id}`.

En tiempo de ejecución ([`coast run`](RUN.md)), estos tarballs se cargan en el [daemon DinD](RUNTIMES_AND_SERVICES.md) interno mediante `docker load`. Esto es lo que permite que las instancias de Coast se inicien rápidamente sin necesidad de descargar imágenes desde un registro.

## Builds e instancias

Cuando ejecutas [`coast run`](RUN.md), Coast resuelve la build más reciente (o un `--build-id` específico) y usa sus artefactos para crear la instancia. El ID de build queda registrado en la instancia.

No necesitas volver a construir para crear más instancias. Una build puede servir a muchas instancias de Coast ejecutándose en paralelo.

## Cuándo reconstruir

Solo vuelve a construir cuando cambien tu Coastfile, `docker-compose.yml` o la configuración de infraestructura. Reconstruir consume muchos recursos -- vuelve a descargar imágenes, vuelve a construir imágenes de Docker y vuelve a extraer secretos.

Los cambios de código no requieren una reconstrucción. Coast monta tu directorio de proyecto directamente en cada instancia, por lo que las actualizaciones de código se reflejan inmediatamente.

## Eliminación automática

Coast conserva hasta 5 builds por tipo de Coastfile. Después de cada `coast build` exitoso, las builds antiguas que excedan el límite se eliminan automáticamente.

Las builds que están en uso por instancias en ejecución nunca se eliminan, independientemente del límite. Si tienes 7 builds pero 3 de ellas respaldan instancias activas, las 3 quedan protegidas.

## Eliminación manual

Puedes eliminar builds manualmente mediante `coast rm-build` o a través de la pestaña Builds de Coastguard.

- **Eliminación completa del proyecto** (`coast rm-build <project>`) requiere que primero todas las instancias se detengan y eliminen. Elimina todo el directorio de builds, las imágenes de Docker asociadas, los volúmenes y los contenedores.
- **Eliminación selectiva** (por ID de build, disponible en la interfaz de Coastguard) omite las builds que están en uso por instancias en ejecución.

## Builds tipadas

Si tu proyecto usa múltiples Coastfiles (por ejemplo, `Coastfile` para la configuración predeterminada y `Coastfile.snap` para volúmenes inicializados con snapshot), cada tipo mantiene su propio enlace simbólico `latest-{type}` y su propio grupo de eliminación de 5 builds.

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

Eliminar una build `snap` nunca afecta a las builds `default`, y viceversa.

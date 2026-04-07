# Builds remotas

Las builds remotas se ejecutan en la máquina remota a través de coast-service. Esto garantiza que la build use la arquitectura nativa del remoto (p. ej., x86_64 en una instancia EC2) independientemente de tu arquitectura local (p. ej., ARM Mac). No se necesita compilación cruzada ni emulación de arquitectura.

## Cómo funciona

Cuando ejecutas `coast build --type remote`, ocurre lo siguiente:

1. El daemon sincroniza mediante rsync los archivos fuente del proyecto (Coastfile, compose.yml, Dockerfiles, inject/) al espacio de trabajo remoto a través de SSH.
2. El daemon llama a `POST /build` en coast-service a través del túnel SSH.
3. coast-service ejecuta la build completa de forma nativa en el remoto: `docker build`, extracción de imágenes, caché de imágenes y extracción de secretos, todo bajo `/data/images/`.
4. coast-service devuelve un `BuildResponse` con la ruta del artefacto y los metadatos de la build.
5. El daemon sincroniza mediante rsync el directorio completo del artefacto (coastfile.toml, compose.yml, manifest.json, secrets/, inject/, image tarballs) de vuelta a `~/.coast/images/{project}/{build_id}/` en tu máquina local.
6. El daemon crea un symlink `latest-remote` que apunta a la nueva build.

```text
Local Machine                              Remote Machine
┌─────────────────────────────┐            ┌───────────────────────────┐
│  ~/.coast/images/my-app/    │            │  /data/images/my-app/     │
│    latest-remote -> {id}    │  ◀─rsync─  │    {id}/                  │
│    {id}/                    │            │      manifest.json        │
│      manifest.json          │            │      coastfile.toml       │
│      coastfile.toml         │            │      compose.yml          │
│      compose.yml            │            │      *.tar (images)       │
│      *.tar (images)         │            │                           │
└─────────────────────────────┘            └───────────────────────────┘
```

## Comandos

```bash
# Build on the default remote (auto-selected if only one registered)
coast build --type remote

# Build on a specific remote
coast build --type remote --remote my-vm

# Build without running (standalone)
coast build --type remote
```

`coast run --type remote` también dispara una build si todavía no existe una build compatible.

## Coincidencia de arquitectura

El `manifest.json` de cada build registra la arquitectura para la que fue construida (p. ej., `aarch64`, `x86_64`). Cuando ejecutas `coast run --type remote`, el daemon comprueba si una build existente coincide con la arquitectura del remoto de destino:

- **La arquitectura coincide**: la build se reutiliza. No es necesario reconstruir.
- **La arquitectura no coincide**: el daemon busca la build más reciente con la arquitectura correcta. Si no existe ninguna, devuelve un error con instrucciones para reconstruir.

Esto significa que puedes construir una vez en un remoto x86_64 y desplegar en cualquier número de remotos x86_64 sin reconstruir. Pero no puedes usar una build ARM en un remoto x86_64 ni viceversa.

## Symlinks

Las builds remotas usan un symlink separado de las builds locales:

| Symlink | Apunta a |
|---------|-----------|
| `latest` | Build local más reciente |
| `latest-remote` | Build remota más reciente |
| `latest-{type}` | Build local más reciente de un tipo específico de Coastfile |

La separación evita que una build remota sobrescriba tu symlink local `latest` o viceversa.

## Poda automática

Coast conserva hasta 5 builds remotas por cada par `(coastfile_type, architecture)`. Después de cada build remota exitosa, las builds más antiguas que excedan el límite se eliminan automáticamente.

Las builds que están en uso por instancias en ejecución nunca se podan, independientemente del límite. Si tienes 7 builds remotas x86_64 pero 3 de ellas respaldan instancias activas, las 3 quedan protegidas.

La poda tiene en cuenta la arquitectura: si tienes builds remotas tanto `aarch64` como `x86_64`, cada arquitectura mantiene su propio grupo de 5 builds de forma independiente.

## Almacenamiento de artefactos

Los artefactos de builds remotas se almacenan en dos lugares:

| Location | Path | Purpose |
|----------|------|---------|
| Remote | `/data/images/{project}/{build_id}/` | Fuente de verdad en la máquina remota |
| Local | `~/.coast/images/{project}/{build_id}/` | Caché local para reutilización entre remotos |

La caché de imágenes en `/data/image-cache/` en el remoto se comparte entre todos los proyectos, al igual que `~/.coast/image-cache/` localmente.

## Relación con las builds locales

Las builds remotas y las builds locales son independientes. Un `coast build` (sin `--type remote`) siempre construye en tu máquina local y actualiza el symlink `latest`. Un `coast build --type remote` siempre construye en la máquina remota y actualiza el symlink `latest-remote`.

Puedes tener coexistiendo builds locales y remotas del mismo proyecto. Los coasts locales usan builds locales; los coasts remotos usan builds remotas.

Para más información sobre cómo funcionan las builds en general (estructura del manifiesto, caché de imágenes, builds tipadas), consulta [Builds](../concepts_and_terminology/BUILDS.md).

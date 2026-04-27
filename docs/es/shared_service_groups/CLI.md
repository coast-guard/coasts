# Referencia de la CLI `coast ssg`

Cada subcomando de `coast ssg` se comunica con el mismo daemon local a través del socket Unix existente. `coast shared-service-group` es un alias de `coast ssg`.

La mayoría de los verbos resuelven un proyecto a partir del `[coast].name` del `Coastfile` del cwd (o `--working-dir <dir>`). Solo `coast ssg ls` es entre proyectos.

Todos los comandos aceptan una bandera global `--silent` / `-s` que suprime la salida de progreso y muestra solo el resumen final o los errores.

## Comandos

### Construir e inspeccionar

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Analiza `Coastfile.shared_service_groups`, extrae cualquier `[secrets.*]`, descarga imágenes, escribe el artefacto en `~/.coast/ssg/<project>/builds/<id>/`, actualiza `latest_build_id`, elimina compilaciones antiguas. Ver [Building](BUILDING.md). |
| `coast ssg ps` | Muestra la lista de servicios de la compilación SSG de este proyecto (lee `manifest.json` más el estado en vivo de los contenedores). Ver [Lifecycle -> ps](LIFECYCLE.md#coast-ssg-ps). |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | Lista cada artefacto de compilación bajo `~/.coast/ssg/<project>/builds/` con marca de tiempo, cantidad de servicios y anotaciones `(latest)` / `(pinned)`. |
| `coast ssg ls` | Listado entre proyectos de cada SSG conocido por el daemon (proyecto, estado, id de compilación, cantidad de servicios, creado en). Ver [Lifecycle -> ls](LIFECYCLE.md#coast-ssg-ls). |

### Ciclo de vida

| Command | Summary |
|---------|---------|
| `coast ssg run` | Crea el DinD `<project>-ssg`, asigna puertos de host dinámicos, materializa secretos (cuando se declaran), inicia la pila compose interna. Ver [Lifecycle -> run](LIFECYCLE.md#coast-ssg-run). |
| `coast ssg start` | Inicia un SSG previamente creado pero detenido. Vuelve a materializar secretos y vuelve a generar cualquier socat de checkout de puerto canónico preservado. |
| `coast ssg stop [--force]` | Detiene el DinD SSG del proyecto. Conserva el contenedor, puertos dinámicos, puertos virtuales y filas de checkout. `--force` desmonta primero los túneles SSH remotos. |
| `coast ssg restart` | Detener + iniciar. Conserva el contenedor y los puertos dinámicos. |
| `coast ssg rm [--with-data] [--force]` | Elimina el DinD SSG del proyecto. `--with-data` elimina los volúmenes con nombre internos. `--force` continúa a pesar de consumidores shadow remotos. Nunca se tocan los contenidos de bind-mount del host. **Nunca se toca el keystore** -- usa `coast ssg secrets clear` para eso. |

### Logs y exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | Transmite logs desde el DinD externo o un servicio interno. `--follow` transmite hasta Ctrl+C. |
| `coast ssg exec [--service <name>] -- <cmd...>` | Ejecuta en el contenedor externo `<project>-ssg` o en un servicio interno. Todo lo que aparece después de `--` se pasa literalmente. |

### Enrutamiento y checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | Muestra el mapeo de puertos canónicos / dinámicos / virtuales por servicio con anotación `(checked out)` donde corresponda. Ver [Routing](ROUTING.md). |
| `coast ssg checkout [--service <name> \| --all]` | Vincula puertos canónicos del host mediante socat del lado del host (el forwarder apunta al puerto virtual estable del proyecto). Desplaza a los poseedores de instancias Coast con una advertencia; falla con procesos de host desconocidos. Ver [Checkout](CHECKOUT.md). |
| `coast ssg uncheckout [--service <name> \| --all]` | Desmonta los socats de puertos canónicos para este proyecto. No restaura automáticamente los Coast desplazados. |

### Diagnóstico

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | Comprobación de solo lectura sobre permisos de bind-mount del host para servicios de imágenes conocidas y secretos SSG declarados pero no extraídos. Emite hallazgos `ok` / `warn` / `info`. Ver [Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor). |

### Fijación de compilación

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | Fija el SSG de este proyecto a un `build_id` específico. `coast ssg run` y `coast build` usan la fijación en lugar de `latest_build_id`. Ver [Building -> Locking a project to a specific build](BUILDING.md#locking-a-project-to-a-specific-build). |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | Libera la fijación. Idempotente. |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | Muestra la fijación actual para este proyecto, si existe. |

### Secretos nativos de SSG

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | Elimina cada entrada cifrada del keystore bajo `coast_image = "ssg:<project>"`. Idempotente. El único verbo que borra secretos nativos de SSG -- `coast ssg rm` y `rm --with-data` deliberadamente los dejan intactos. Ver [Secrets](SECRETS.md). |

### Asistente de migración

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Resuelve el punto de montaje de un volumen con nombre Docker del host y emite (o aplica) la entrada equivalente de bind-mount de SSG. Ver [Volumes -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume). |

## Códigos de salida

- `0` -- éxito. Comandos como `doctor` devuelven 0 incluso cuando encuentran advertencias; son herramientas de diagnóstico, no puertas de control.
- Distinto de cero -- error de validación, error de Docker, inconsistencia de estado o rechazo por puerta de shadow remoto.

## Ver también

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Routing](ROUTING.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Secrets](SECRETS.md)
- [Checkout](CHECKOUT.md)

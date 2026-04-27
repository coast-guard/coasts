# Ciclo de vida del SSG

El SSG de cada proyecto es su propio contenedor externo Docker-in-Docker llamado `<project>-ssg` (p. ej. `cg-ssg`). Los verbos de ciclo de vida apuntan al SSG del proyecto al que pertenezca el `Coastfile` del cwd (o el proyecto nombrado mediante `--working-dir`). Cada comando mutante se serializa mediante un mutex por proyecto en el daemon, de modo que dos invocaciones concurrentes de `coast ssg run` / `coast ssg stop` contra el mismo proyecto se encolan en lugar de competir -- pero dos proyectos distintos pueden mutar sus SSG en paralelo.

## Máquina de estados

```text
                     coast ssg build           coast ssg run
(no build)   -->  built     -->     created    -->     running
                                                          |
                                                   coast ssg stop
                                                          v
                                                       stopped
                                                          |
                                                  coast ssg start
                                                          v
                                                       running
                                                          |
                                                   coast ssg rm
                                                          v
                                                      (removed)
```

- `coast ssg build` no crea un contenedor. Produce un artefacto en disco bajo `~/.coast/ssg/<project>/builds/<id>/` y (cuando se declara `[secrets.*]`) extrae los valores de los secretos al keystore.
- `coast ssg run` crea el DinD `<project>-ssg`, asigna puertos dinámicos del host, materializa cualquier secreto declarado en un `compose.override.yml` por ejecución, y arranca la pila compose interna.
- `coast ssg stop` detiene el DinD externo pero conserva el contenedor, las filas de puertos dinámicos y los puertos virtuales por proyecto para que `start` sea rápido.
- `coast ssg start` vuelve a levantar el SSG y vuelve a materializar secretos (de modo que un `coast ssg secrets clear` entre stop y start surte efecto).
- `coast ssg rm` elimina el contenedor DinD externo. Con `--with-data` también elimina los volúmenes nombrados internos (el contenido de los bind mounts del host nunca se toca). El keystore nunca se borra con `rm` -- solo `coast ssg secrets clear` hace eso.
- `coast ssg restart` es un contenedor práctico para `stop` + `start`.

## Comandos

### `coast ssg run`

Crea el DinD `<project>-ssg` si no existe e inicia sus servicios internos. Asigna un puerto dinámico del host por cada servicio declarado y los publica en el DinD externo. Escribe las asignaciones en la base de datos de estado para que el asignador de puertos no los reutilice.

```bash
coast ssg run
```

Transmite eventos de progreso mediante el mismo canal `BuildProgressEvent` que `coast ssg build`. El plan predeterminado tiene 7 pasos:

1. Preparando SSG
2. Creando contenedor SSG
3. Iniciando contenedor SSG
4. Esperando al daemon interno
5. Cargando imágenes en caché
6. Materializando secretos (silencioso cuando no hay bloque `[secrets]`; de lo contrario emite elementos por secreto)
7. Iniciando servicios internos

**Inicio automático**. `coast run` en un Coast consumidor que referencia un servicio SSG inicia automáticamente el SSG si aún no está en ejecución. Siempre puedes ejecutar `coast ssg run` explícitamente, pero rara vez lo necesitas. Consulta [Consuming -> Auto-start](CONSUMING.md#auto-start).

### `coast ssg start`

Inicia un SSG previamente detenido. Requiere un contenedor `<project>-ssg` existente (es decir, un `coast ssg run` previo). Vuelve a materializar secretos desde el keystore para que cualquier cambio desde la detención surta efecto, luego vuelve a levantar los socats de checkout del lado del host para cualquier puerto canónico que hubiera sido reservado antes de la detención.

```bash
coast ssg start
```

### `coast ssg stop`

Detiene el contenedor DinD externo. La pila compose interna se apaga con él. Se conservan el contenedor, las asignaciones de puertos dinámicos y las filas de puertos virtuales por proyecto para que el siguiente `start` sea rápido.

```bash
coast ssg stop
coast ssg stop --force
```

Los socats de checkout del lado del host se terminan, pero sus filas en la base de datos de estado sobreviven. El siguiente `coast ssg start` o `coast ssg run` los vuelve a levantar. Consulta [Checkout](CHECKOUT.md).

**Puerta de consumidores remotos.** El daemon se niega a detener el SSG mientras cualquier Coast shadow remoto (uno creado con `coast assign --remote ...`) lo esté consumiendo actualmente. Pasa `--force` para desmontar los túneles SSH inversos y continuar de todos modos. Consulta [Consuming -> Remote Coasts](CONSUMING.md#remote-coasts).

### `coast ssg restart`

Equivale a `stop` + `start`. Conserva el contenedor y las asignaciones de puertos dinámicos.

```bash
coast ssg restart
```

### `coast ssg rm`

Elimina el contenedor DinD externo. De forma predeterminada, esto conserva los volúmenes nombrados internos (Postgres WAL, etc.), por lo que tus datos sobreviven entre ciclos de `rm` / `run`. El contenido de los bind mounts del host nunca se toca.

```bash
coast ssg rm                    # conserva volúmenes nombrados; conserva keystore
coast ssg rm --with-data        # también elimina volúmenes nombrados; aún conserva keystore
coast ssg rm --force            # continúa a pesar de consumidores remotos
```

- `--with-data` elimina todos los volúmenes nombrados internos antes de eliminar el propio DinD. Usa esto cuando quieras una base de datos limpia.
- `--force` continúa incluso cuando Coasts shadow remotos referencian el SSG. Misma semántica que `stop --force`.
- `rm` limpia las filas `ssg_port_checkouts` (destructivo sobre los bindings del host de puertos canónicos).

El keystore -- donde viven los secretos nativos del SSG (`coast_image = "ssg:<project>"`) -- **no** se ve afectado por `rm` ni por `rm --with-data`. Para borrar secretos del SSG, usa `coast ssg secrets clear` (consulta [Secrets](SECRETS.md)).

### `coast ssg ps`

Muestra el estado de los servicios del SSG del proyecto actual. Lee `manifest.json` para la configuración construida, luego inspecciona la base de datos de estado activa para obtener metadatos de contenedores en ejecución.

```bash
coast ssg ps
```

Salida después de un `run` exitoso:

```text
SSG build: b455787d95cfdeb_20260420061903  (project: cg, running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

Muestra la asignación de puertos canónico / dinámico / virtual por servicio, con una anotación `(checked out)` cuando hay un socat activo del lado del host para el puerto canónico de ese servicio. El puerto virtual es al que realmente se conectan los consumidores. Consulta [Routing](ROUTING.md) para más detalles.

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
#   postgres             5432            54201           42000      (checked out)
#   redis                6379            54202           42001
```

### `coast ssg logs`

Transmite logs del contenedor DinD externo o de un servicio interno específico.

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>` apunta a un servicio interno por clave de compose; sin él obtienes stdout del DinD externo.
- `--tail N` limita las líneas históricas (predeterminado 200).
- `--follow` / `-f` transmite nuevas líneas a medida que llegan, hasta `Ctrl+C`.

### `coast ssg exec`

Ejecuta un comando dentro del DinD externo o de un servicio interno.

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- Sin `--service`, el comando se ejecuta en el contenedor externo `<project>-ssg`.
- Con `--service <name>`, el comando se ejecuta dentro de ese servicio compose mediante `docker compose exec -T`.
- Todo lo que va después de `--` se pasa al `docker exec` subyacente, incluidas las flags.

### `coast ssg ls`

Lista todos los SSG conocidos por el daemon, en todos los proyectos. Este es el único verbo que no resuelve un proyecto desde el cwd; devuelve filas para cada entrada en el estado SSG del daemon.

```bash
coast ssg ls

#   PROJECT     STATUS     BUILD                                       SERVICES   CREATED
#   cg          running    b455787d95cfdeb_20260420061903               2          2026-04-20T06:19:03Z
#   filemap     stopped    b9b93fdb41b21337_20260418123012               3          2026-04-18T12:30:12Z
```

Útil para detectar SSG olvidados de proyectos antiguos, o para ver rápidamente qué proyectos en esta máquina tienen un SSG en cualquier estado.

## Semántica del mutex

Cada verbo mutante de SSG (`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`) adquiere un mutex SSG por proyecto dentro del daemon antes de despachar al manejador real. Dos invocaciones concurrentes contra el mismo proyecto se encolan; contra proyectos distintos se ejecutan en paralelo. Los verbos de solo lectura (`ps`/`ports`/`logs`/`exec`/`doctor`/`ls`) no adquieren el mutex.

## Integración con Coastguard

Si estás ejecutando [Coastguard](../concepts_and_terminology/COASTGUARD.md), la SPA representa el ciclo de vida del SSG en su propia página (`/project/<p>/ssg/local`) con pestañas para Exec, Ports, Services, Logs, Secrets, Stats, Images y Volumes. `CoastEvent::SsgStarting` y `CoastEvent::SsgStarted` se disparan siempre que un Coast consumidor activa un inicio automático, para que la UI pueda atribuir el arranque al proyecto que lo necesitó.

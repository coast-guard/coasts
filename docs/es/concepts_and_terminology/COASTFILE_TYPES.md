# Tipos de Coastfile

Un solo proyecto puede tener múltiples Coastfiles para diferentes casos de uso. Cada variante se llama un "tipo". Los tipos te permiten componer configuraciones que comparten una base común pero difieren en qué servicios se ejecutan, cómo se manejan los volúmenes o si los servicios se inician automáticamente.

## Cómo funcionan los tipos

La convención de nombres es `Coastfile` para el predeterminado y `Coastfile.{type}` para las variantes. El sufijo después del punto se convierte en el nombre del tipo:

- `Coastfile` -- tipo predeterminado
- `Coastfile.test` -- tipo de prueba
- `Coastfile.snap` -- tipo de instantánea
- `Coastfile.light` -- tipo ligero

Cualquier Coastfile puede tener una extensión `.toml` opcional para el resaltado de sintaxis en el editor. El sufijo `.toml` se elimina antes de derivar el tipo, por lo que estos son pares equivalentes:

- `Coastfile.toml` = `Coastfile` (tipo predeterminado)
- `Coastfile.test.toml` = `Coastfile.test` (tipo de prueba)
- `Coastfile.light.toml` = `Coastfile.light` (tipo ligero)

**Regla de desempate:** si existen ambas formas (p. ej. `Coastfile` y `Coastfile.toml`, o `Coastfile.light` y `Coastfile.light.toml`), la variante `.toml` tiene prioridad.

**Nombres de tipo reservados:** `"default"` y `"toml"` no pueden usarse como nombres de tipo. `Coastfile.default` y `Coastfile.toml` (como sufijo de tipo, es decir, un archivo literalmente llamado `Coastfile.toml.toml`) se rechazan.

Construyes y ejecutas Coasts tipados con `--type`:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

Un Coastfile tipado hereda de un padre mediante `extends`. Todo lo del padre se fusiona. El hijo solo necesita especificar lo que sobrescribe o agrega.

```toml
[coast]
extends = "Coastfile"
```

Esto evita duplicar toda tu configuración para cada variante. El hijo hereda todos los [ports](PORTS.md), [secrets](SECRETS.md), [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), [assign strategies](ASSIGN.md), comandos de configuración y configuraciones de [MCP](MCP_SERVERS.md) del padre. Cualquier cosa que defina el hijo tiene prioridad sobre el padre.

## [unset]

Elimina elementos específicos heredados del padre por nombre. Puedes eliminar `ports`, `shared_services`, `secrets` y `volumes`.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

Así es como una variante de prueba elimina servicios compartidos (para que las bases de datos se ejecuten dentro del Coast con volúmenes aislados) y quita puertos que no necesita.

## [omit]

Elimina completamente servicios de compose de la compilación. Los servicios omitidos se eliminan del archivo compose y no se ejecutan dentro del Coast en absoluto.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

Usa esto para excluir servicios que son irrelevantes para el propósito de la variante. Una variante de prueba podría conservar solo la base de datos, las migraciones y el ejecutor de pruebas.

## autostart

Controla si `docker compose up` se ejecuta automáticamente cuando se inicia el Coast. El valor predeterminado es `true`.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

Establece `autostart = false` para variantes en las que quieras ejecutar comandos específicos manualmente en lugar de levantar toda la pila. Esto es común para ejecutores de pruebas -- creas el Coast y luego usas [`coast exec`](EXEC_AND_DOCKER.md) para ejecutar suites de prueba individuales.

## Patrones comunes

### Variante de prueba

Un `Coastfile.test` que conserva solo lo necesario para ejecutar pruebas:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

Cada Coast de prueba obtiene su propia base de datos limpia. No se expone ningún puerto porque las pruebas se comunican con los servicios a través de la red interna de compose. `autostart = false` significa que activas las ejecuciones de prueba manualmente con `coast exec`.

### Variante de instantánea

Un `Coastfile.snap` que inicializa cada Coast con una copia de los volúmenes de base de datos existentes del host:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

Los servicios compartidos se eliminan para que las bases de datos se ejecuten dentro de cada Coast. `snapshot_source` inicializa los volúmenes aislados a partir de volúmenes existentes del host en el momento de la compilación. Después de la creación, los datos de cada instancia divergen de forma independiente.

### Variante ligera

Un `Coastfile.light` que reduce el proyecto al mínimo para un flujo de trabajo específico -- quizás solo un servicio backend y su base de datos para una iteración rápida.

## Pools de compilación independientes

Cada tipo tiene su propio enlace simbólico `latest-{type}` y su propio pool de poda automática de 5 compilaciones:

```bash
coast build              # updates latest, prunes default builds
coast build --type test  # updates latest-test, prunes test builds
coast build --type snap  # updates latest-snap, prunes snap builds
```

Compilar un tipo `test` no afecta las compilaciones `default` o `snap`. La poda es completamente independiente por tipo.

## Ejecutar Coasts tipados

Las instancias creadas con `--type` se etiquetan con su tipo. Puedes tener instancias de diferentes tipos ejecutándose simultáneamente para el mismo proyecto:

```bash
coast run dev-1                    # default type
coast run test-1 --type test       # test type
coast run snapshot-1 --type snap   # snapshot type

coast ls
# All three appear, each with their own type, ports, and volume strategy
```

Así es como puedes tener un entorno de desarrollo completo ejecutándose junto con ejecutores de pruebas aislados e instancias inicializadas desde instantáneas, todo para el mismo proyecto, todo al mismo tiempo.

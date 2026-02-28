# Tipos de Coastfile

Un solo proyecto puede tener múltiples Coastfiles para diferentes casos de uso. Cada variante se llama un "tipo". Los tipos te permiten componer configuraciones que comparten una base común pero difieren en qué servicios se ejecutan, cómo se manejan los volúmenes o si los servicios se inician automáticamente.

## Cómo funcionan los tipos

La convención de nombres es `Coastfile` para el predeterminado y `Coastfile.{type}` para las variantes. El sufijo después del punto se convierte en el nombre del tipo:

- `Coastfile` -- tipo predeterminado
- `Coastfile.test` -- tipo de pruebas
- `Coastfile.snap` -- tipo de instantánea
- `Coastfile.light` -- tipo ligero

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

Esto evita duplicar toda tu configuración para cada variante. El hijo hereda todos los [puertos](PORTS.md), [secretos](SECRETS.md), [volúmenes](VOLUMES.md), [servicios compartidos](SHARED_SERVICES.md), [estrategias de asignación](ASSIGN.md), comandos de configuración y configuraciones de [MCP](MCP_SERVERS.md) del padre. Cualquier cosa que el hijo defina tiene prioridad sobre el padre.

## [unset]

Elimina elementos específicos heredados del padre por nombre. Puedes hacer unset de `ports`, `shared_services`, `secrets` y `volumes`.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

Así es como una variante de pruebas elimina servicios compartidos (para que las bases de datos se ejecuten dentro del Coast con volúmenes aislados) y elimina puertos que no necesita.

## [omit]

Elimina por completo servicios de compose de la construcción. Los servicios omitidos se eliminan del archivo compose y no se ejecutan dentro del Coast en absoluto.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

Úsalo para excluir servicios que son irrelevantes para el propósito de la variante. Una variante de pruebas podría mantener solo la base de datos, las migraciones y el ejecutor de pruebas.

## autostart

Controla si `docker compose up` se ejecuta automáticamente cuando inicia el Coast. El valor predeterminado es `true`.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

Establece `autostart = false` para variantes en las que quieres ejecutar comandos específicos manualmente en lugar de levantar toda la pila. Esto es común para ejecutores de pruebas: creas el Coast y luego usas [`coast exec`](EXEC_AND_DOCKER.md) para ejecutar suites de pruebas individuales.

## Patrones comunes

### Variante de pruebas

Un `Coastfile.test` que mantiene solo lo necesario para ejecutar pruebas:

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

Cada Coast de pruebas obtiene su propia base de datos limpia. No se exponen puertos porque las pruebas se comunican con los servicios a través de la red interna de compose. `autostart = false` significa que disparas las ejecuciones de pruebas manualmente con `coast exec`.

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

Los servicios compartidos se eliminan para que las bases de datos se ejecuten dentro de cada Coast. `snapshot_source` inicializa los volúmenes aislados a partir de volúmenes existentes del host en el momento de la construcción. Después de la creación, los datos de cada instancia divergen de forma independiente.

### Variante ligera

Un `Coastfile.light` que reduce el proyecto al mínimo para un flujo de trabajo específico: quizá solo un servicio de backend y su base de datos para iteración rápida.

## Grupos de construcción independientes

Cada tipo tiene su propio enlace simbólico `latest-{type}` y su propio grupo de auto-poda de 5 construcciones:

```bash
coast build              # actualiza latest, poda las construcciones predeterminadas
coast build --type test  # actualiza latest-test, poda las construcciones de test
coast build --type snap  # actualiza latest-snap, poda las construcciones de snap
```

Construir un tipo `test` no afecta a las construcciones `default` o `snap`. La poda es completamente independiente por tipo.

## Ejecutar Coasts tipados

Las instancias creadas con `--type` se etiquetan con su tipo. Puedes tener instancias de diferentes tipos ejecutándose simultáneamente para el mismo proyecto:

```bash
coast run dev-1                    # tipo predeterminado
coast run test-1 --type test       # tipo de pruebas
coast run snapshot-1 --type snap   # tipo de instantánea

coast ls
# Las tres aparecen, cada una con su propio tipo, puertos y estrategia de volúmenes
```

Así es como puedes tener un entorno de desarrollo completo ejecutándose junto a ejecutores de pruebas aislados e instancias inicializadas desde instantáneas, todo para el mismo proyecto, todo al mismo tiempo.

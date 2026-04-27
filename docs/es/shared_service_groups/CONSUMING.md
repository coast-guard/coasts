# Consumo de un Grupo de Servicios Compartidos

Un Coast consumidor opta por los servicios propiedad del SSG de su proyecto, servicio por servicio, usando una bandera de una sola línea en el `Coastfile` del consumidor. Dentro del Coast, los contenedores de la app siguen viendo `postgres:5432`; la capa de enrutamiento del daemon redirige ese tráfico al DinD externo `<project>-ssg` del proyecto mediante un puerto virtual estable.

El SSG al que hace referencia `from_group = true` es **siempre el SSG del propio proyecto consumidor**. No existe compartición entre proyectos. Si el `[coast].name` del consumidor es `cg`, `from_group = true` se resuelve contra `Coastfile.shared_service_groups` de `cg-ssg`.

## Sintaxis

Agrega un bloque `[shared_services.<name>]` con `from_group = true`:

```toml
# Consumer Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = true       # overrides the SSG service's default
```

La clave TOML (`postgres` en este ejemplo) debe coincidir con un nombre de servicio declarado en `Coastfile.shared_service_groups` del proyecto.

## Campos Prohibidos

Con `from_group = true`, los siguientes campos se rechazan en tiempo de parseo:

- `image`
- `ports`
- `env`
- `volumes`

Todos estos viven del lado del SSG. Si alguno aparece junto con `from_group = true`, `coast build` falla con:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

## Overrides Permitidos

Dos campos siguen siendo legales por consumidor:

- `inject` -- la variable de entorno o ruta de archivo mediante la cual se expone la cadena de conexión. Distintos proyectos consumidores pueden exponer la misma forma bajo diferentes nombres de variables de entorno.
- `auto_create_db` -- si Coast debe crear una base de datos por instancia dentro de este servicio en tiempo de `coast run`. Sobrescribe el valor `auto_create_db` propio del servicio SSG.

## Detección de Conflictos

Dos bloques `[shared_services.<name>]` con el mismo nombre en un único Coastfile se rechazan en tiempo de parseo. Esa regla se mantiene.

Un bloque con `from_group = true` que hace referencia a un nombre no declarado en `Coastfile.shared_service_groups` del proyecto falla en tiempo de `coast build`:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

Esta es la verificación de errores tipográficos. No hay una verificación separada de "drift" en tiempo de ejecución -- las discrepancias de forma entre el consumidor y el SSG se manifiestan en la verificación en tiempo de compilación, y cualquier discrepancia adicional en tiempo de ejecución aflora de forma natural como un error de conexión desde la perspectiva de la app.

## Auto-inicio

`coast run` en un consumidor inicia automáticamente el SSG del proyecto cuando todavía no está en ejecución:

- Existe el build del SSG, el contenedor no está en ejecución -> el daemon ejecuta el equivalente de `coast ssg start` (o `run` si el contenedor nunca fue creado), protegido por el mutex SSG del proyecto.
- No existe ningún build del SSG -> error fatal:

  ```text
  Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
  ```

- El SSG ya está en ejecución -> no-op, `coast run` continúa inmediatamente.

Los eventos de progreso `SsgStarting` y `SsgStarted` se disparan en el stream de ejecución para que [Coastguard](../concepts_and_terminology/COASTGUARD.md) pueda atribuir el arranque al proyecto consumidor.

## Cómo Funciona el Enrutamiento

Dentro de un Coast consumidor, el contenedor de la app resuelve `postgres:5432` al SSG del proyecto mediante tres piezas:

1. **IP alias + `extra_hosts`** agregan `postgres -> <docker0 alias IP>` al compose interno del consumidor, de modo que las búsquedas DNS de `postgres` tengan éxito.
2. **socat en DinD** escucha en `<alias>:5432` y reenvía a `host.docker.internal:<virtual_port>`. El puerto virtual es estable para `(project, service, container_port)` -- no cambia cuando el SSG se recompila.
3. **socat del host** en `<virtual_port>` reenvía a `127.0.0.1:<dynamic>`, donde `<dynamic>` es el puerto actualmente publicado del contenedor SSG. El socat del host se actualiza cuando el SSG se recompila; el socat en DinD del consumidor nunca tiene que cambiar.

El código de la app y el DNS de compose no cambian. Migrar un proyecto de Postgres inline a Postgres en SSG es una pequeña edición del Coastfile (eliminar `image`/`ports`/`env`, agregar `from_group = true`) más una recompilación.

Para el recorrido completo salto por salto, los conceptos de puertos y la justificación, consulta [Routing](ROUTING.md).

## `auto_create_db`

`auto_create_db = true` en un servicio Postgres o MySQL de SSG hace que el daemon cree una base de datos `{instance}_{project}` dentro de ese servicio para cada Coast consumidor que se ejecuta. El nombre de la base de datos coincide con lo que produce el patrón inline `[shared_services]`, de modo que las URLs de `inject` concuerdan con la base de datos que crea `auto_create_db`.

La creación es idempotente. Volver a ejecutar `coast run` en una instancia cuya base de datos ya existe es un no-op. El SQL subyacente es idéntico al de la ruta inline, por lo que la salida DDL es byte por byte la misma independientemente del patrón que use tu proyecto.

Un consumidor puede sobrescribir el valor `auto_create_db` del servicio SSG:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject` expone una cadena de conexión al contenedor de la app. Mismo formato que [Secrets](../coastfiles/SECRETS.md): `"env:NAME"` crea una variable de entorno, `"file:/path"` escribe un archivo dentro del contenedor coast del consumidor y lo monta mediante bind como solo lectura en cada servicio del compose interno no stubbed.

La cadena resuelta usa el nombre de servicio canónico y el puerto canónico, no el puerto dinámico del host. Esa invariancia es todo el punto -- los contenedores de la app siempre ven `postgres://coast:coast@postgres:5432/{db}` independientemente de qué puerto dinámico esté publicando el SSG.

Tanto `env:NAME` como `file:/path` están completamente implementados.

Este `inject` es el pipeline de secretos **del lado del consumidor**: el valor se calcula a partir de metadatos canónicos del SSG en tiempo de `coast build` y se inyecta en el DinD coast del consumidor. Es independiente del pipeline `[secrets.*]` **del lado del SSG** (consulta [Secrets](SECRETS.md)) que extrae valores para que los *propios* servicios del SSG los consuman.

## Coasts Remotos

Un Coast remoto (uno creado con `coast assign --remote ...`) alcanza un SSG local mediante un túnel SSH inverso. El daemon local genera `ssh -N -R <vport>:localhost:<vport>` desde la máquina remota de vuelta al puerto virtual local; dentro del DinD remoto, `extra_hosts: postgres: host-gateway` resuelve `postgres` a la IP host-gateway del remoto, y el túnel SSH coloca el SSG local al otro lado con el mismo número de puerto virtual.

Ambos lados del túnel usan el puerto **virtual**, no el puerto dinámico. Esto significa que recompilar el SSG localmente nunca invalida el túnel remoto.

Los túneles se coalescen por `(project, remote_host, service, container_port)` -- múltiples instancias consumidoras del mismo proyecto en el mismo remoto comparten un único proceso `ssh -R`. Eliminar un consumidor no desmonta el túnel; solo lo hace la eliminación del último consumidor.

Consecuencias prácticas:

- `coast ssg stop` / `rm` se niegan mientras un shadow Coast remoto esté consumiendo actualmente el SSG. El daemon lista los shadows que bloquean para que sepas qué está usando el SSG.
- `coast ssg stop --force` (o `rm --force`) desmonta primero el `ssh -R` compartido y luego continúa. Usa esto cuando aceptas que los consumidores remotos perderán conectividad.

Consulta [Routing](ROUTING.md) para la arquitectura completa del túnel remoto y [Remote Coasts](../remote_coasts/README.md) para la configuración más general de máquina remota.

## Ver También

- [Routing](ROUTING.md) -- conceptos de puertos canónico / dinámico / virtual y la cadena de enrutamiento completa
- [Secrets](SECRETS.md) -- `[secrets.*]` nativo del SSG para credenciales del lado del servicio (ortogonal a `inject` del lado del consumidor)
- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- esquema completo de `[shared_services.*]` incluyendo `from_group = true`
- [Lifecycle](LIFECYCLE.md) -- qué hace `coast run` tras bambalinas, incluido el auto-inicio
- [Checkout](CHECKOUT.md) -- binding en el lado del host de puertos canónicos para herramientas ad-hoc
- [Volumes](VOLUMES.md) -- montajes y permisos; relevante cuando recompilas el SSG y la nueva imagen de Postgres cambia la propiedad del directorio de datos

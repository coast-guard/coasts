# Ejecución

`coast run` crea una nueva instancia de Coast. Resuelve la [build](BUILDS.md) más reciente, aprovisiona un [contenedor DinD](RUNTIMES_AND_SERVICES.md), carga imágenes en caché, inicia tus servicios de compose, asigna [puertos dinámicos](PORTS.md) y registra la instancia en la base de datos de estado.

```bash
coast run dev-1
```

Si pasas `-w`, Coast también [asigna](ASSIGN.md) el worktree después de que se complete el aprovisionamiento:

```bash
coast run dev-1 -w feature/oauth
```

Este es el patrón más común cuando un harness o agente crea un worktree y necesita un Coast para él en un solo paso.

## Qué sucede

`coast run` ejecuta cuatro fases:

1. **Validar e insertar** — comprueba que el nombre sea único, resuelve el ID de build (desde el symlink `latest` o un `--build-id` explícito) e inserta un registro de instancia `Provisioning`.
2. **Aprovisionamiento de Docker** — crea el contenedor DinD en el daemon del host, construye cualquier imagen por instancia, carga tarballs de imágenes en caché en el daemon interno, reescribe el archivo compose, inyecta secretos y ejecuta `docker compose up -d`.
3. **Finalizar** — almacena las asignaciones de puertos, establece el puerto principal si hay exactamente uno y transiciona la instancia a `Running`.
4. **Asignación opcional de worktree** — si se proporcionó `-w <worktree>`, ejecuta `coast assign` contra la nueva instancia. Si la asignación falla, el Coast sigue en ejecución — el fallo se registra como una advertencia.

El volumen persistente `/var/lib/docker` dentro del contenedor DinD significa que las ejecuciones posteriores omiten la carga de imágenes. Un `coast run` nuevo con cachés frías puede tardar más de 20 segundos; una nueva ejecución después de `coast rm` normalmente termina en menos de 10 segundos.

## Uso de CLI

```text
coast run <name> [options]
```

| Flag | Description |
|------|-------------|
| `-w`, `--worktree <name>` | Asigna este worktree después de que se complete el aprovisionamiento |
| `--n <count>` | Creación por lotes. El nombre debe contener `{n}` (p. ej. `coast run dev-{n} --n=5` crea dev-1 hasta dev-5) |
| `-t`, `--type <type>` | Usa una build tipada (p. ej. `--type snap` resuelve `latest-snap` en lugar de `latest`) |
| `--force-remove-dangling` | Elimina un contenedor Docker sobrante con el mismo nombre antes de crear |
| `-s`, `--silent` | Suprime la salida de progreso; solo imprime el resumen final o los errores |
| `-v`, `--verbose` | Muestra detalles verbosos, incluidos los logs de build de Docker |

La rama git siempre se detecta automáticamente a partir del HEAD actual.

## Creación por lotes

Usa `{n}` en el nombre y `--n` para crear múltiples instancias a la vez:

```bash
coast run dev-{n} --n=5
```

Esto crea `dev-1`, `dev-2`, `dev-3`, `dev-4`, `dev-5` secuencialmente. Cada instancia obtiene su propio contenedor DinD, asignaciones de puertos y estado de volumen. Los lotes de más de 10 solicitan confirmación.

## Builds tipadas

Si tu proyecto usa múltiples tipos de Coastfile (consulta [Tipos de Coastfile](COASTFILE_TYPES.md)), pasa `--type` para seleccionar qué build usar:

```bash
coast run dev-1                    # resolves "latest"
coast run test-1 --type test       # resolves "latest-test"
coast run snapshot-1 --type snap   # resolves "latest-snap"
```

## Run vs assign y remove

- `coast run` crea una instancia **nueva**. Úsalo cuando necesites otro Coast.
- `coast assign` redirige una instancia **existente** a un worktree diferente. Úsalo
  cuando ya tengas un Coast y quieras cambiar qué código ejecuta.
- `coast rm` desmonta una instancia por completo. Úsalo cuando quieras apagar
  Coasts o recrear uno desde cero.

La mayoría de los cambios cotidianos no necesitan `coast rm`; `coast assign` y
`coast checkout` suelen ser suficientes. Recurre a `coast rm` cuando quieras una
recreación limpia, especialmente después de reconstruir tu Coastfile o build.

Puedes combinarlos: `coast run dev-3 -w feature/billing` crea la instancia
y asigna el worktree en un solo paso.

## Contenedores colgantes

Si un `coast run` anterior fue interrumpido o `coast rm` no limpió completamente, es posible que veas un error de "contenedor Docker colgante". Pasa `--force-remove-dangling` para eliminar el contenedor sobrante y continuar:

```bash
coast run dev-1 --force-remove-dangling
```

## Ver también

- [Remove](REMOVE.md) — desmontar una instancia por completo
- [Builds](BUILDS.md) — lo que consume `coast run`
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — la arquitectura DinD dentro de cada instancia
- [Assign and Unassign](ASSIGN.md) — cambiar una instancia existente a un worktree diferente
- [Ports](PORTS.md) — cómo se asignan los puertos dinámicos y canónicos
- [Coasts](COASTS.md) — el concepto de alto nivel de una instancia Coast

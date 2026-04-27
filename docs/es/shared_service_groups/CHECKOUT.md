# Checkout del SSG en el lado del host

Los Consumer Coasts alcanzan los servicios del SSG a través de la capa de enrutamiento del daemon (socat in-DinD -> socat del host -> puerto dinámico). Eso funciona muy bien para los contenedores de aplicaciones. No ayuda a quienes llaman desde el lado del host -- MCPs, sesiones ad-hoc de `psql`, el inspector de bases de datos de tu editor -- que quieren conectarse a `localhost:5432` como si el servicio viviera justo ahí.

`coast ssg checkout` resuelve eso. Genera un socat a nivel de host que enlaza el puerto canónico del host (5432 para Postgres, 6379 para Redis, ...) y reenvía al puerto virtual estable del proyecto. Desde ahí, el socat de puerto virtual existente del host transporta el tráfico hacia el puerto dinámico actualmente publicado del SSG.

Todo esto es por proyecto. `coast ssg checkout --service postgres` se resuelve al proyecto que posee el `Coastfile` del cwd; si tienes dos proyectos en esta máquina, solo uno puede mantener el puerto canónico 5432 a la vez.

## Uso

```bash
coast ssg checkout --service postgres     # bind one service
coast ssg checkout --all                  # bind every SSG service
coast ssg uncheckout --service postgres   # tear down one
coast ssg uncheckout --all                # tear down every active checkout
```

Después de un checkout exitoso, `coast ssg ports` anota cada servicio enlazado con `(checked out)`:

```text
  SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
  postgres             5432            54201           42000      (checked out)
  redis                6379            54202           42001
```

Los Consumer Coasts siempre alcanzan los servicios del SSG a través de su cadena socat in-DinD -> puerto virtual, independientemente del estado del checkout en el lado del host. El checkout es puramente una conveniencia del lado del host.

## Reenviador de dos saltos

El socat del checkout **no** apunta directamente al puerto dinámico del host del SSG. Apunta al puerto virtual estable del proyecto:

```text
host process            -> 127.0.0.1:5432           (checkout socat, listens here)
                        -> 127.0.0.1:42000          (project's virtual port)
                        -> 127.0.0.1:54201          (SSG's current dynamic port)
                        -> <project>-ssg postgres   (inner service)
```

La cadena de dos saltos significa que el socat del checkout sigue funcionando entre reconstrucciones del SSG aunque el puerto dinámico cambie. Solo se actualiza el socat del puerto virtual del host -- el socat del puerto canónico no se entera. Consulta [Routing](ROUTING.md) para ver cómo se mantiene la capa socat del host.

## Desplazamiento de poseedores de instancias de Coast

Cuando le pides al SSG que haga checkout de un puerto canónico, ese puerto puede ya estar ocupado. La semántica depende de quién lo tenga:

- **Una instancia de Coast que fue explícitamente checked out.** `coast checkout <instance>` en algún Coast hoy más temprano enlazó `localhost:5432` al Postgres interno de ese Coast. El checkout del SSG **lo desplaza**: el daemon mata el socat existente, limpia `port_allocations.socat_pid` para ese Coast, y enlaza en su lugar el socat del SSG. La CLI imprime una advertencia clara:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432 -> virtual 42000.
  ```

  El Coast desplazado **no** se vuelve a enlazar automáticamente cuando después haces `coast ssg uncheckout`. Su puerto dinámico sigue funcionando, pero el puerto canónico permanece sin enlazar hasta que ejecutes `coast checkout my-app/dev-2` otra vez.

- **El checkout del SSG de otro proyecto.** Si `filemap-ssg` ya tiene 5432 checked out y tratas de hacer checkout del 5432 de `cg-ssg`, el daemon se niega con un mensaje claro que nombra al poseedor. Haz uncheckout del 5432 de `filemap-ssg` primero.

- **Una fila previa de checkout del SSG con un `socat_pid` muerto.** Metadatos obsoletos de un daemon que falló o de un ciclo de stop/start. El nuevo checkout recupera silenciosamente la fila.

- **Cualquier otra cosa** (un Postgres del host que iniciaste a mano, otro daemon, `nginx` en el puerto 8080). `coast ssg checkout` falla:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  No existe una bandera `--force`. Se consideró demasiado peligroso matar silenciosamente un proceso desconocido.

## Comportamiento de Stop / Start

`coast ssg stop` mata los procesos socat activos del puerto canónico pero **preserva las propias filas de checkout** en la base de datos de estado.

`coast ssg run` / `start` / `restart` iteran las filas preservadas y vuelven a generar un socat fresco de puerto canónico por fila. El puerto canónico (5432) permanece idéntico; solo cambia el puerto dinámico entre ciclos de `run`, y como el socat del checkout apunta al puerto **virtual** (que también es estable), el reenlace es mecánico.

Si un servicio desaparece del SSG reconstruido, su fila de checkout se elimina con una advertencia en la respuesta de ejecución:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm` borra todas las filas `ssg_port_checkouts` del proyecto. `rm` es destructivo por diseño -- pediste explícitamente una pizarra limpia.

## Recuperación tras reinicio del daemon

Después de un reinicio inesperado del daemon (fallo, `coastd restart`, reboot), `restore_running_state` consulta la tabla `ssg_port_checkouts` y vuelve a generar cada fila contra la asignación actual de puerto dinámico / virtual. Tu `localhost:5432` permanece enlazado a través de los vaivenes del daemon.

## Cuándo hacer checkout

- Quieres apuntar un cliente GUI de base de datos al Postgres del SSG del proyecto.
- Quieres que `psql "postgres://coast:coast@localhost:5432/mydb"` funcione sin descubrir primero el puerto dinámico.
- Un MCP en tu host necesita un endpoint canónico estable.
- Coastguard quiere actuar como proxy del puerto HTTP de administración del SSG.

Cuándo **no** hacer checkout:

- Para conectividad desde dentro de un Consumer Coast -- eso ya funciona a través de socat in-DinD al puerto virtual.
- Cuando te basta con usar la salida de `coast ssg ports` y conectar el puerto dinámico en tu herramienta.

## Ver también

- [Routing](ROUTING.md) -- los conceptos de puertos canónico / dinámico / virtual y la cadena completa de reenviadores del lado del host
- [Lifecycle](LIFECYCLE.md) -- detalles de stop / start / rm
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- la versión para instancias de Coast de esta idea
- [Ports](../concepts_and_terminology/PORTS.md) -- el cableado de puertos canónicos vs dinámicos en todo el sistema

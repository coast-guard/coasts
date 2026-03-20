# Eliminar

`coast rm` desmantela por completo una instancia de Coast. Detiene la instancia si
está en ejecución, elimina el contenedor DinD, borra los volúmenes aislados, libera
los puertos, elimina los shells del agente y borra la instancia del estado.

```bash
coast rm dev-1
```

La mayoría de los flujos de trabajo cotidianos no necesitan `coast rm`. Si solo quieres que un Coast
ejecute código diferente o posea los puertos canónicos, usa [Assign and
Unassign](ASSIGN.md) o [Checkout](CHECKOUT.md) en su lugar. Recurre a `coast rm`
cuando quieras desactivar Coasts, recuperar el estado de ejecución por instancia o
recrear una instancia desde cero después de reconstruir tu Coastfile o compilación.

## Qué sucede

`coast rm` ejecuta cinco fases:

1. **Validar y localizar** — busca la instancia en el estado. Si el registro
   del estado ya no existe pero todavía existe un contenedor colgante con el nombre esperado,
   `coast rm` también limpia eso.
2. **Detener si es necesario** — si la instancia está `Running` o `CheckedOut`, Coast
   primero baja la pila compose interna y detiene el contenedor DinD.
3. **Eliminar artefactos de ejecución** — elimina el contenedor Coast y borra
   los volúmenes aislados de esa instancia.
4. **Limpiar el estado del host** — termina los redireccionadores de puertos persistentes, libera
   los puertos, elimina los shells del agente y borra el registro de la instancia de la base de datos
   de estado.
5. **Conservar los datos compartidos** — los volúmenes de servicios compartidos y los datos de servicios compartidos
   se dejan intactos.

## Uso de la CLI

```text
coast rm <name>
coast rm --all
```

| Flag | Description |
|------|-------------|
| `<name>` | Eliminar una instancia por nombre |
| `--all` | Eliminar todas las instancias del proyecto actual |

`coast rm --all` resuelve el proyecto actual, enumera sus instancias y las elimina
una por una. Si no hay instancias, sale correctamente.

## Servicios compartidos y compilaciones

- `coast rm` **no** elimina los datos de servicios compartidos.
- Usa `coast shared-services rm <service>` si también quieres eliminar un servicio compartido
  y sus datos.
- Usa `coast rm-build` si quieres eliminar los artefactos de compilación después de desactivar
  instancias.

## Cuándo usarlo

- después de reconstruir tu Coastfile o crear una nueva compilación y querer una
  instancia nueva
- cuando quieras desactivar Coasts y liberar el estado de contenedores y volúmenes
  por instancia
- cuando una instancia está atascada y empezar de nuevo es más fácil que depurarla
  en su lugar

## Ver también

- [Run](RUN.md) — crear una nueva instancia de Coast
- [Assign and Unassign](ASSIGN.md) — redirigir una instancia existente a un
  worktree diferente
- [Shared Services](SHARED_SERVICES.md) — lo que `coast rm` no elimina
- [Builds](BUILDS.md) — artefactos de compilación y `coast rm-build`

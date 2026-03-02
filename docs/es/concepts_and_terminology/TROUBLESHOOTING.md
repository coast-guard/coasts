# Solución de problemas

La mayoría de los problemas con Coasts provienen de estado obsoleto, recursos Docker huérfanos o un daemon que se desincronizó. Esta página cubre la ruta de escalamiento de leve a nuclear.

## Doctor

Si algo se siente raro — las instancias aparecen como en ejecución pero nada responde, los puertos parecen atascados o la UI muestra datos obsoletos — comienza con `coast doctor`:

```bash
coast doctor
```

Doctor analiza la base de datos de estado y Docker en busca de inconsistencias: registros de instancias huérfanos con contenedores faltantes, contenedores colgantes sin registro de estado y servicios compartidos marcados como en ejecución que en realidad están caídos. Arregla automáticamente lo que encuentra.

Para previsualizar lo que haría sin cambiar nada:

```bash
coast doctor --dry-run
```

## Reinicio del daemon

Si el daemon en sí parece no responder o sospechas que está en un mal estado, reinícialo:

```bash
coast daemon restart
```

Esto envía una señal de apagado ordenado, espera a que el daemon termine y arranca un proceso nuevo. Tus instancias y el estado se conservan.

## Eliminar un solo proyecto

Si el problema está aislado a un proyecto, puedes eliminar sus artefactos de build y los recursos Docker asociados sin afectar nada más:

```bash
coast rm-build my-project
```

Esto elimina el directorio de artefactos del proyecto, imágenes Docker, volúmenes y contenedores. Primero pide confirmación. Pasa `--force` para omitir el aviso.

## Restablecimiento de fábrica con Nuke

Cuando nada más funciona — o simplemente quieres una pizarra completamente limpia — `coast nuke` realiza un restablecimiento de fábrica completo:

```bash
coast nuke
```

Esto hará lo siguiente:

1. Detener el daemon `coastd`.
2. Eliminar **todos** los contenedores Docker gestionados por coast.
3. Eliminar **todos** los volúmenes Docker gestionados por coast.
4. Eliminar **todas** las redes Docker gestionadas por coast.
5. Eliminar **todas** las imágenes Docker de coast.
6. Borrar por completo el directorio `~/.coast/` (base de datos de estado, builds, logs, secretos, caché de imágenes).
7. Recrear `~/.coast/` y reiniciar el daemon para que coast vuelva a estar utilizable de inmediato.

Como esto destruye todo, debes escribir `nuke` en el indicador de confirmación:

```text
$ coast nuke
WARNING: This will permanently destroy ALL coast data:

  - Stop the coastd daemon
  - Remove all coast-managed Docker containers
  - Remove all coast-managed Docker volumes
  - Remove all coast-managed Docker networks
  - Remove all coast Docker images
  - Delete ~/.coast/ (state DB, builds, logs, secrets, image cache)

Type "nuke" to confirm:
```

Pasa `--force` para omitir el aviso (útil en scripts):

```bash
coast nuke --force
```

Después de un nuke, coast queda listo para usarse — el daemon está ejecutándose y el directorio home existe. Solo necesitas volver a ejecutar `coast build` y `coast run` en tus proyectos.

## Reportar errores

Si encuentras un problema que no se resuelve con nada de lo anterior, incluye los logs del daemon al reportarlo:

```bash
coast daemon logs
```

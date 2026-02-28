# Daemon de Coast

El daemon de Coast (`coastd`) es el proceso local de larga duración que realiza el trabajo real de orquestación. La [CLI](CLI.md) y [Coastguard](COASTGUARD.md) son clientes; `coastd` es el plano de control detrás de ellos.

## Arquitectura de un Vistazo

```text
coast CLI (automation) -----+
                            +--> coastd daemon
Coastguard UI (human) ------+         |
                                      +--> Coasts
                                      +--> Ports
                                      +--> State
```

La CLI envía solicitudes a `coastd` a través de un socket Unix local; Coastguard se conecta a través de un WebSocket. El daemon aplica cambios al estado en tiempo de ejecución.

## Qué Hace

`coastd` maneja las operaciones que necesitan estado persistente y coordinación en segundo plano:

- Rastrea instancias de Coast, compilaciones y servicios compartidos.
- Crea, inicia, detiene y elimina entornos de ejecución de Coast.
- Aplica operaciones de asignar/desasignar/checkout.
- Gestiona el [reenvío de puertos](PORTS.md) canónico y dinámico.
- Transmite [logs](LOGS.md), estado y eventos de ejecución a clientes CLI y UI.

En resumen: si ejecutas `coast run`, `coast assign`, `coast checkout` o `coast ls`, el daemon es el componente que hace el trabajo.

## Cómo se Ejecuta

Puedes ejecutar el daemon de dos formas comunes:

```bash
# Register daemon auto-start at login (recommended)
coast daemon install

# Manual start mode
coast daemon start
```

Si omites la instalación del daemon, necesitas iniciarlo tú mismo en cada sesión antes de usar comandos de Coast.

## Reportar Errores

Si te encuentras con problemas, incluye los logs del daemon `coastd` al enviar un informe de error. Los logs contienen el contexto necesario para diagnosticar la mayoría de los problemas:

```bash
coast daemon logs
```

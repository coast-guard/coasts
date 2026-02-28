# Coastguard

Coastguard es la interfaz web local de Coast (piensa: una interfaz al estilo Docker Desktop de Coast), ejecutándose en el puerto `31415`. Se inicia desde la CLI:

```bash
coast ui
```

![Coastguard project overview](../../assets/coastguard-overview.png)
*El panel del proyecto que muestra instancias de Coast en ejecución, sus ramas/worktrees y el estado de checkout.*

![Coastguard port mappings](../../assets/coastguard-ports.png)
*La página de puertos para una instancia específica de Coast, mostrando mapeos de puertos canónicos y dinámicos para cada servicio.*

## Para Qué Es Útil Coastguard

Coastguard te ofrece una superficie visual de control y observabilidad para tu proyecto:

- Ver proyectos, instancias, estados, ramas y estado de checkout.
- Inspeccionar los [mapeos de puertos](PORTS.md) y saltar directamente a los servicios.
- Ver [logs](LOGS.md), estadísticas de ejecución e inspeccionar datos.
- Explorar [builds](BUILDS.md), artefactos de imágenes, metadatos de [volúmenes](VOLUMES.md) y [secrets](SECRETS.md).
- Navegar la documentación dentro de la app mientras trabajas.

## Relación con la CLI y el Daemon

Coastguard no reemplaza la CLI. La complementa como la interfaz orientada a humanos.

- La [CLI `coast`](CLI.md) es la interfaz de automatización para scripts, flujos de trabajo de agentes e integraciones con herramientas.
- Coastguard es la interfaz humana para inspección visual, depuración interactiva y visibilidad operativa del día a día.
- Ambos son clientes de [`coastd`](DAEMON.md), por lo que se mantienen sincronizados.

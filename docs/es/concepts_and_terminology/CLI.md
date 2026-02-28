# Coast CLI

La Coast CLI (`coast`) es la interfaz principal de línea de comandos para operar Coasts. Es intencionalmente ligera: analiza tu comando, envía una solicitud a [`coastd`](DAEMON.md) e imprime salida estructurada de vuelta en tu terminal.

## Para Qué La Usas

Los flujos de trabajo típicos se ejecutan desde la CLI:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Coasts
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

La CLI también incluye comandos de documentación que son útiles para humanos y agentes:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## Por Qué Existe por Separado del Daemon

Separar la CLI del daemon te ofrece algunos beneficios importantes:

- El daemon mantiene estado y procesos de larga duración.
- La CLI se mantiene rápida, componible y fácil de automatizar con scripts.
- Puedes ejecutar comandos puntuales sin mantener vivo el estado del terminal.
- Las herramientas de agentes pueden invocar comandos de la CLI de formas predecibles y aptas para la automatización.

## CLI vs Coastguard

Usa la interfaz que mejor se adapte al momento:

- La CLI está diseñada para una cobertura operativa completa: cualquier cosa que puedas hacer en Coastguard también debería ser posible desde la CLI.
- Trata la CLI como la interfaz de automatización — scripts, flujos de trabajo de agentes, trabajos de CI y herramientas personalizadas para desarrolladores.
- Trata [Coastguard](COASTGUARD.md) como la interfaz humana — inspección visual, depuración interactiva y visibilidad operativa.

Ambas se comunican con el mismo daemon, por lo que operan sobre el mismo estado subyacente del proyecto.

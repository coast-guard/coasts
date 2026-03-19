# Documentación de Coasts

```youtube
MBGKSKau4sU
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## Instalación

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*Si decides no ejecutar `coast daemon install`, eres responsable de iniciar el daemon manualmente con `coast daemon start` cada vez.*

## ¿Qué son Coasts?

Un Coast (**host contenedorizado**) es un runtime local de desarrollo. Coasts te permiten ejecutar múltiples entornos aislados para el mismo proyecto en una sola máquina.

Coasts son especialmente útiles para stacks complejos de `docker-compose` con muchos servicios interdependientes, pero son igualmente eficaces para configuraciones locales de desarrollo no contenedorizadas. Coasts admiten una amplia gama de [patrones de configuración de runtime](concepts_and_terminology/RUNTIMES_AND_SERVICES.md) para que puedas diseñar el entorno ideal para múltiples agentes trabajando en paralelo.

Coasts están construidos para el desarrollo local, no como un servicio cloud alojado. Tus entornos se ejecutan localmente en tu máquina.

El proyecto Coasts es software gratuito, local, con licencia MIT, agnóstico al proveedor de agentes y agnóstico al arnés de agentes, sin ventas adicionales de IA.

Coasts funcionan con cualquier flujo de trabajo de programación agéntica que use worktrees. No se requiere ninguna configuración especial del lado del arnés.

## Por qué Coasts para Worktrees

Los worktrees de Git son excelentes para aislar cambios de código, pero por sí solos no resuelven el aislamiento del runtime.

Cuando ejecutas múltiples worktrees en paralelo, rápidamente te encuentras con problemas de ergonomía:

- [Conflictos de puertos](concepts_and_terminology/PORTS.md) entre servicios que esperan los mismos puertos del host.
- Configuración de base de datos y [volúmenes](concepts_and_terminology/VOLUMES.md) por worktree que es tediosa de gestionar.
- Entornos de pruebas de integración que necesitan cableado de runtime personalizado por worktree.
- El infierno viviente de cambiar de worktree y reconstruir el contexto del runtime cada vez. Ver [Asignar y Desasignar](concepts_and_terminology/ASSIGN.md).

Si Git es control de versiones para tu código, Coasts son como Git para los runtimes de tus worktrees.

Cada entorno obtiene sus propios puertos, así que puedes inspeccionar el runtime de cualquier worktree en paralelo. Cuando [haces checkout](concepts_and_terminology/CHECKOUT.md) de un runtime de worktree, Coasts remapean ese runtime a los puertos canónicos de tu proyecto.

Coasts abstraen la configuración del runtime en una capa modular simple encima de los worktrees, para que cada worktree pueda ejecutarse con el aislamiento que necesita sin mantener manualmente una configuración compleja por worktree.

## Requisitos

- macOS o Linux
- Docker Desktop en macOS, o Docker Engine con el plugin Compose en Linux
- Un proyecto que use Git
- Node.js
- `socat` (`brew install socat` en macOS, `sudo apt install socat` en Ubuntu)

```text
Nota sobre Linux: Los puertos dinámicos funcionan de inmediato en Linux.
Si necesitas puertos canónicos por debajo de `1024`, consulta la documentación de checkout para la configuración de host necesaria.
```

## ¿Contenerizar agentes?

Puedes contenerizar un agente con un Coast. Eso podría sonar como una gran idea al principio, pero en muchos casos en realidad no necesitas ejecutar tu agente de programación dentro de un contenedor.

Debido a que Coasts comparten el [sistema de archivos](concepts_and_terminology/FILESYSTEM.md) con tu máquina host mediante un montaje de volumen compartido, el flujo de trabajo más fácil y fiable es ejecutar el agente en tu host e indicarle que ejecute tareas pesadas de runtime (como pruebas de integración) dentro de la instancia de Coast usando [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md).

Sin embargo, si sí quieres ejecutar tu agente en un contenedor, Coasts lo soportan absolutamente mediante [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md). Puedes construir un rig increíblemente intrincado para esta configuración, incluyendo la [configuración del servidor MCP](concepts_and_terminology/MCP_SERVERS.md), pero puede que no interopere limpiamente con el software de orquestación que existe hoy. Para la mayoría de los flujos de trabajo, los agentes del lado del host son más simples y más fiables.

## Coasts vs Dev Containers

Coasts no son dev containers, y no son lo mismo.

Los dev containers generalmente están diseñados para montar un IDE dentro de un único espacio de trabajo de desarrollo contenedorizado. Coasts son headless y están optimizados como entornos ligeros para el uso de agentes en paralelo con worktrees — múltiples entornos de runtime aislados y conscientes de worktrees ejecutándose lado a lado, con cambios rápidos de checkout y controles de aislamiento del runtime para cada instancia.

## Repo de demostración

Si quieres un pequeño proyecto de ejemplo para probar con Coasts, empieza con el repositorio [`coasts-demo`](https://github.com/coast-guard/coasts-demo).

## Coasts Video Course

Si prefieres el video, el [Coasts Video Course](learn-coasts-videos/README.md) cubre cada concepto central en menos de tres minutos cada uno.

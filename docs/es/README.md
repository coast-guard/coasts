# Documentación de Coasts

## Instalación

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*Si decides no ejecutar `coast daemon install`, eres responsable de iniciar el daemon manualmente con `coast daemon start` cada vez.*

## ¿Qué son Coasts?

Un Coast (**host en contenedor**) es un entorno de ejecución local para desarrollo. Coasts te permiten ejecutar múltiples entornos aislados para el mismo proyecto en una sola máquina.

Coasts son especialmente útiles para stacks complejos de `docker-compose` con muchos servicios interdependientes, pero son igual de efectivos para configuraciones de desarrollo local no containerizadas. Coasts admiten una amplia gama de [patrones de configuración de runtime](concepts_and_terminology/RUNTIMES_AND_SERVICES.md) para que puedas dar forma al entorno ideal para múltiples agentes trabajando en paralelo.

Coasts están diseñados para el desarrollo local, no como un servicio cloud alojado. Tus entornos se ejecutan localmente en tu máquina.

El proyecto Coasts es software gratuito, local, con licencia MIT, agnóstico respecto al proveedor de agentes y agnóstico respecto al arnés de agentes, sin ventas adicionales de IA.

Coasts funcionan con cualquier flujo de trabajo de programación agentica que use worktrees. No se requiere ninguna configuración especial del lado del arnés.

## Por qué Coasts para Worktrees

Los worktrees de Git son excelentes para aislar cambios de código, pero por sí solos no resuelven el aislamiento del entorno de ejecución.

Cuando ejecutas múltiples worktrees en paralelo, rápidamente te encuentras con problemas de ergonomía:

- [Conflictos de puertos](concepts_and_terminology/PORTS.md) entre servicios que esperan los mismos puertos del host.
- Configuración de base de datos por worktree y [configuración de volúmenes](concepts_and_terminology/VOLUMES.md) que es tediosa de gestionar.
- Entornos de pruebas de integración que necesitan cableado de runtime personalizado por worktree.
- El infierno viviente de cambiar de worktree y reconstruir el contexto del entorno de ejecución cada vez. Consulta [Assign and Unassign](concepts_and_terminology/ASSIGN.md).

Si Git es el control de versiones para tu código, Coasts son como Git para los entornos de ejecución de tus worktrees.

Cada entorno obtiene sus propios puertos, por lo que puedes inspeccionar cualquier entorno de ejecución de worktree en paralelo. Cuando [haces checkout](concepts_and_terminology/CHECKOUT.md) de un entorno de ejecución de worktree, Coasts remapean ese entorno a los puertos canónicos de tu proyecto.

Coasts abstraen la configuración del entorno de ejecución en una capa modular simple sobre los worktrees, para que cada worktree pueda ejecutarse con el aislamiento que necesita sin mantener manualmente configuraciones complejas por worktree.

## Requisitos

- macOS
- Docker Desktop
- Un proyecto que use Git
- Node.js
- `socat` *(instalado con `curl -fsSL https://coasts.dev/install | sh` como una dependencia `depends_on` de Homebrew)*

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## ¿Containerizar agentes?

Puedes containerizar un agente con un Coast. Eso puede sonar como una gran idea al principio, pero en muchos casos en realidad no necesitas ejecutar tu agente de programación dentro de un contenedor.

Debido a que Coasts comparten el [sistema de archivos](concepts_and_terminology/FILESYSTEM.md) con tu máquina host mediante un montaje de volumen compartido, el flujo de trabajo más fácil y fiable es ejecutar el agente en tu host e indicarle que ejecute tareas intensivas de runtime (como pruebas de integración) dentro de la instancia de Coast usando [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md).

Sin embargo, si sí quieres ejecutar tu agente en un contenedor, Coasts lo soportan totalmente mediante [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md). Puedes construir un rig increíblemente intrincado para esta configuración, incluyendo [configuración del servidor MCP](concepts_and_terminology/MCP_SERVERS.md), pero puede que no interoperé limpiamente con el software de orquestación que existe hoy. Para la mayoría de los flujos de trabajo, los agentes del lado del host son más simples y más fiables.

## Coasts vs Dev Containers

Coasts no son dev containers, y no son lo mismo.

Los dev containers generalmente están diseñados para montar un IDE dentro de un único espacio de trabajo de desarrollo containerizado. Coasts son headless y están optimizados como entornos ligeros para el uso paralelo de agentes con worktrees — múltiples entornos de ejecución aislados y conscientes del worktree ejecutándose lado a lado, con cambios de checkout rápidos y controles de aislamiento del entorno de ejecución para cada instancia.

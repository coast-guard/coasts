# Conceptos y Terminología

Esta sección cubre los conceptos centrales y el vocabulario utilizado en todo Coasts. Si eres nuevo en Coasts, empieza aquí antes de profundizar en la configuración o el uso avanzado.

- [Coasts](COASTS.md) - tiempos de ejecución autocontenidos de tu proyecto, cada uno con sus propios puertos, volúmenes y asignación de worktree.
- [Run](RUN.md) - crear una nueva instancia de Coast a partir de la compilación más reciente, asignando opcionalmente un worktree.
- [Remove](REMOVE.md) - desmontar una instancia de Coast y su estado de ejecución aislado cuando necesitas una recreación limpia o quieres detener Coasts.
- [Filesystem](FILESYSTEM.md) - el montaje compartido entre el host y Coast, los agentes del lado del host y el cambio de worktree.
- [Private Paths](PRIVATE_PATHS.md) - aislamiento por instancia para rutas del espacio de trabajo que entran en conflicto a través de montajes bind compartidos.
- [Coast Daemon](DAEMON.md) - el plano de control local `coastd` que ejecuta operaciones del ciclo de vida.
- [Coast CLI](CLI.md) - la interfaz de terminal para comandos, scripts y flujos de trabajo de agentes.
- [Coastguard](COASTGUARD.md) - la interfaz web iniciada con `coast ui` para observabilidad y control.
- [Ports](PORTS.md) - puertos canónicos frente a puertos dinámicos y cómo checkout alterna entre ellos.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - enlaces rápidos a tu servicio principal, enrutamiento por subdominios para aislamiento de cookies y plantillas de URL.
- [Assign and Unassign](ASSIGN.md) - cambiar un Coast entre worktrees y las estrategias de asignación disponibles.
- [Checkout](CHECKOUT.md) - asignar puertos canónicos a una instancia de Coast y cuándo lo necesitas.
- [Lookup](LOOKUP.md) - descubrir qué instancias de Coast coinciden con el worktree actual del agente.
- [Volume Topology](VOLUMES.md) - servicios compartidos, volúmenes compartidos, volúmenes aislados y creación de snapshots.
- [Shared Services](SHARED_SERVICES.md) - servicios de infraestructura administrados por el host y desambiguación de volúmenes.
- [Secrets and Extractors](SECRETS.md) - extraer secretos del host e inyectarlos en contenedores de Coast.
- [Builds](BUILDS.md) - la anatomía de una compilación de coast, dónde viven los artefactos, poda automática y compilaciones tipadas.
- [Coastfile Types](COASTFILE_TYPES.md) - variantes componibles de Coastfile con extends, unset, omit y autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) - el tiempo de ejecución DinD, la arquitectura Docker-in-Docker y cómo se ejecutan los servicios dentro de un Coast.
- [Bare Services](BARE_SERVICES.md) - ejecutar procesos no contenedorizados dentro de un Coast y por qué deberías contenedorizarlos en su lugar.
- [Bare Service Optimization](BARE_SERVICE_OPTIMIZATION.md) - instalaciones condicionales, caché, private_paths, conectividad con servicios compartidos y estrategias de asignación para servicios bare.
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - las variables `<SERVICE>_DYNAMIC_PORT` inyectadas automáticamente y cómo usarlas en comandos de servicio.
- [Logs](LOGS.md) - leer registros de servicio desde dentro de un Coast, la compensación de MCP y el visor de registros de Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) - ejecutar comandos dentro de un Coast y comunicarse con el daemon interno de Docker.
- [Agent Shells](AGENT_SHELLS.md) - TUIs de agentes contenedorizados, la compensación de OAuth y por qué probablemente deberías ejecutar agentes en el host en su lugar.
- [MCP Servers](MCP_SERVERS.md) - configurar herramientas MCP dentro de un Coast para agentes contenedorizados, servidores internos frente a servidores proxy del host.
- [Remotes](REMOTES.md) - ejecutar servicios en una máquina remota mediante coast-service mientras se mantiene sin cambios el flujo de trabajo local.
- [Troubleshooting](TROUBLESHOOTING.md) - doctor, reinicio del daemon, eliminación del proyecto y la opción nuclear de restablecimiento de fábrica.

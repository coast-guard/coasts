# Conceptos y terminología

Esta sección cubre los conceptos fundamentales y el vocabulario utilizados en todo Coasts. Si eres nuevo en Coasts, empieza aquí antes de profundizar en la configuración o en el uso avanzado.

- [Coasts](COASTS.md) — runtimes autocontenidos de tu proyecto, cada uno con sus propios puertos, volúmenes y asignación de worktree.
- [Filesystem](FILESYSTEM.md) — el montaje compartido entre el host y un Coast, los agentes del lado del host y el cambio de worktree.
- [Coast Daemon](DAEMON.md) — el plano de control local `coastd` que ejecuta operaciones del ciclo de vida.
- [Coast CLI](CLI.md) — la interfaz de terminal para comandos, scripts y flujos de trabajo de agentes.
- [Coastguard](COASTGUARD.md) — la UI web iniciada con `coast ui` para observabilidad y control.
- [Ports](PORTS.md) — puertos canónicos vs puertos dinámicos y cómo el checkout intercambia entre ellos.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — enlaces rápidos a tu servicio principal, enrutamiento por subdominio para aislamiento de cookies y plantillas de URL.
- [Assign and Unassign](ASSIGN.md) — cambiar un Coast entre worktrees y las estrategias de asignación disponibles.
- [Checkout](CHECKOUT.md) — mapeo de puertos canónicos a una instancia de Coast y cuándo lo necesitas.
- [Lookup](LOOKUP.md) — descubrir qué instancias de Coast coinciden con el worktree actual del agente.
- [Volume Topology](VOLUMES.md) — servicios compartidos, volúmenes compartidos, volúmenes aislados y creación de snapshots.
- [Shared Services](SHARED_SERVICES.md) — servicios de infraestructura gestionados por el host y desambiguación de volúmenes.
- [Secrets and Extractors](SECRETS.md) — extraer secretos del host e inyectarlos en contenedores de Coast.
- [Builds](BUILDS.md) — la anatomía de una build de coast, dónde viven los artefactos, auto-poda y builds tipadas.
- [Coastfile Types](COASTFILE_TYPES.md) — variantes componibles de Coastfile con extends, unset, omit y autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — el runtime DinD, la arquitectura Docker-in-Docker y cómo se ejecutan los servicios dentro de un Coast.
- [Bare Services](BARE_SERVICES.md) — ejecutar procesos no contenerizados dentro de un Coast y por qué deberías contenerizarlos en su lugar.
- [Logs](LOGS.md) — leer logs de servicios desde dentro de un Coast, el compromiso del MCP y el visor de logs de Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) — ejecutar comandos dentro de un Coast y comunicarse con el daemon Docker interno.
- [Agent Shells](AGENT_SHELLS.md) — TUIs de agentes contenerizadas, el compromiso de OAuth y por qué probablemente deberías ejecutar los agentes en el host en su lugar.
- [MCP Servers](MCP_SERVERS.md) — configurar herramientas MCP dentro de un Coast para agentes contenerizados, servidores internos vs proxificados por el host.

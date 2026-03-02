# Conceptos y Terminología

Esta sección cubre los conceptos centrales y el vocabulario utilizados en todo Coasts. Si eres nuevo en Coasts, empieza aquí antes de profundizar en la configuración o el uso avanzado.

- [Coasts](COASTS.md) — runtimes autocontenidos de tu proyecto, cada uno con sus propios puertos, volúmenes y asignación de worktree.
- [Sistema de archivos](FILESYSTEM.md) — el montaje compartido entre el host y un Coast, agentes del lado del host y cambio de worktree.
- [Daemon de Coast](DAEMON.md) — el plano de control local `coastd` que ejecuta operaciones del ciclo de vida.
- [CLI de Coast](CLI.md) — la interfaz de terminal para comandos, scripts y flujos de trabajo de agentes.
- [Coastguard](COASTGUARD.md) — la UI web lanzada con `coast ui` para observabilidad y control.
- [Puertos](PORTS.md) — puertos canónicos vs puertos dinámicos y cómo checkout intercambia entre ellos.
- [Puerto Primario y DNS](PRIMARY_PORT_AND_DNS.md) — enlaces rápidos a tu servicio primario, enrutamiento por subdominios para aislamiento de cookies y plantillas de URL.
- [Asignar y Desasignar](ASSIGN.md) — cambiar un Coast entre worktrees y las estrategias de asignación disponibles.
- [Checkout](CHECKOUT.md) — mapear puertos canónicos a una instancia de Coast y cuándo lo necesitas.
- [Lookup](LOOKUP.md) — descubrir qué instancias de Coast coinciden con el worktree actual del agente.
- [Topología de Volúmenes](VOLUMES.md) — servicios compartidos, volúmenes compartidos, volúmenes aislados y creación de instantáneas.
- [Servicios Compartidos](SHARED_SERVICES.md) — servicios de infraestructura gestionados por el host y desambiguación de volúmenes.
- [Secretos y Extractores](SECRETS.md) — extraer secretos del host e inyectarlos en contenedores de Coast.
- [Builds](BUILDS.md) — la anatomía de una build de coast, dónde viven los artefactos, auto-purga y builds tipadas.
- [Tipos de Coastfile](COASTFILE_TYPES.md) — variantes composables de Coastfile con extends, unset, omit y autostart.
- [Runtimes y Servicios](RUNTIMES_AND_SERVICES.md) — el runtime DinD, la arquitectura Docker-in-Docker y cómo se ejecutan los servicios dentro de un Coast.
- [Servicios Bare](BARE_SERVICES.md) — ejecutar procesos no contenerizados dentro de un Coast y por qué deberías contenerizarlos en su lugar.
- [Logs](LOGS.md) — leer logs de servicios desde dentro de un Coast, el compromiso de MCP y el visor de logs de Coastguard.
- [Exec y Docker](EXEC_AND_DOCKER.md) — ejecutar comandos dentro de un Coast y hablar con el daemon Docker interno.
- [Shells de Agentes](AGENT_SHELLS.md) — TUIs de agentes contenerizadas, el compromiso de OAuth y por qué probablemente deberías ejecutar agentes en el host en su lugar.
- [Servidores MCP](MCP_SERVERS.md) — configurar herramientas MCP dentro de un Coast para agentes contenerizados, servidores internos vs proxificados por el host.
- [Solución de problemas](TROUBLESHOOTING.md) — doctor, reinicio del daemon, eliminación del proyecto y la opción nuclear de restablecimiento de fábrica.

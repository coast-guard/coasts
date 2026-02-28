# Conceitos e Terminologia

Esta seção aborda os conceitos centrais e o vocabulário usados ao longo do Coasts. Se você é novo no Coasts, comece aqui antes de mergulhar em configuração ou uso avançado.

- [Coasts](COASTS.md) — runtimes autocontidos do seu projeto, cada um com suas próprias portas, volumes e atribuição de worktree.
- [Filesystem](FILESYSTEM.md) — o mount compartilhado entre host e Coast, agentes no lado do host e troca de worktree.
- [Coast Daemon](DAEMON.md) — o plano de controle local `coastd` que executa operações de ciclo de vida.
- [Coast CLI](CLI.md) — a interface de terminal para comandos, scripts e fluxos de trabalho de agentes.
- [Coastguard](COASTGUARD.md) — a UI web iniciada com `coast ui` para observabilidade e controle.
- [Ports](PORTS.md) — portas canônicas vs portas dinâmicas e como o checkout alterna entre elas.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — links rápidos para seu serviço principal, roteamento por subdomínio para isolamento de cookies e modelos de URL.
- [Assign and Unassign](ASSIGN.md) — alternando uma Coast entre worktrees e as estratégias de atribuição disponíveis.
- [Checkout](CHECKOUT.md) — mapeando portas canônicas para uma instância de Coast e quando você precisa disso.
- [Lookup](LOOKUP.md) — descobrindo quais instâncias de Coast correspondem ao worktree atual do agente.
- [Volume Topology](VOLUMES.md) — serviços compartilhados, volumes compartilhados, volumes isolados e snapshotting.
- [Shared Services](SHARED_SERVICES.md) — serviços de infraestrutura gerenciados no host e desambiguação de volumes.
- [Secrets and Extractors](SECRETS.md) — extraindo segredos do host e injetando-os em contêineres Coast.
- [Builds](BUILDS.md) — a anatomia de um build de coast, onde os artefatos vivem, auto-pruning e builds tipados.
- [Coastfile Types](COASTFILE_TYPES.md) — variantes componíveis de Coastfile com extends, unset, omit e autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — o runtime DinD, a arquitetura Docker-in-Docker e como os serviços rodam dentro de uma Coast.
- [Bare Services](BARE_SERVICES.md) — executando processos não conteinerizados dentro de uma Coast e por que você deveria conteinerizar em vez disso.
- [Logs](LOGS.md) — lendo logs de serviço de dentro de uma Coast, o tradeoff do MCP e o visualizador de logs do Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) — executando comandos dentro de uma Coast e conversando com o daemon Docker interno.
- [Agent Shells](AGENT_SHELLS.md) — TUIs de agentes conteinerizadas, o tradeoff do OAuth e por que você provavelmente deveria executar agentes no host em vez disso.
- [MCP Servers](MCP_SERVERS.md) — configurando ferramentas MCP dentro de uma Coast para agentes conteinerizados, servidores internos vs servidores proxyados pelo host.

# Conceitos e Terminologia

Esta seção cobre os conceitos centrais e o vocabulário usados em todo o Coasts. Se você é novo no Coasts, comece aqui antes de mergulhar na configuração ou no uso avançado.

- [Coasts](COASTS.md) — runtimes autocontidos do seu projeto, cada um com suas próprias portas, volumes e atribuição de worktree.
- [Filesystem](FILESYSTEM.md) — a montagem compartilhada entre host e Coast, agentes no lado do host e troca de worktree.
- [Coast Daemon](DAEMON.md) — o plano de controle local `coastd` que executa operações de ciclo de vida.
- [Coast CLI](CLI.md) — a interface de terminal para comandos, scripts e fluxos de trabalho de agentes.
- [Coastguard](COASTGUARD.md) — a UI web iniciada com `coast ui` para observabilidade e controle.
- [Ports](PORTS.md) — portas canônicas vs portas dinâmicas e como o checkout alterna entre elas.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) — links rápidos para seu serviço primário, roteamento por subdomínio para isolamento de cookies e templates de URL.
- [Assign and Unassign](ASSIGN.md) — alternar uma Coast entre worktrees e as estratégias de atribuição disponíveis.
- [Checkout](CHECKOUT.md) — mapear portas canônicas para uma instância de Coast e quando você precisa disso.
- [Lookup](LOOKUP.md) — descobrir quais instâncias de Coast correspondem ao worktree atual do agente.
- [Volume Topology](VOLUMES.md) — serviços compartilhados, volumes compartilhados, volumes isolados e snapshotting.
- [Shared Services](SHARED_SERVICES.md) — serviços de infraestrutura gerenciados pelo host e desambiguação de volumes.
- [Secrets and Extractors](SECRETS.md) — extrair segredos do host e injetá-los em contêineres Coast.
- [Builds](BUILDS.md) — a anatomia de um build do coast, onde os artefatos ficam, auto-pruning e builds tipados.
- [Coastfile Types](COASTFILE_TYPES.md) — variantes de Coastfile componíveis com extends, unset, omit e autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — o runtime DinD, a arquitetura Docker-in-Docker e como os serviços rodam dentro de uma Coast.
- [Bare Services](BARE_SERVICES.md) — executar processos não containerizados dentro de uma Coast e por que você deveria containerizar em vez disso.
- [Logs](LOGS.md) — ler logs de serviços de dentro de uma Coast, o tradeoff do MCP e o visualizador de logs do Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) — executar comandos dentro de uma Coast e falar com o daemon Docker interno.
- [Agent Shells](AGENT_SHELLS.md) — TUIs de agentes em contêiner, o tradeoff do OAuth e por que você provavelmente deveria executar agentes no host em vez disso.
- [MCP Servers](MCP_SERVERS.md) — configurar ferramentas MCP dentro de uma Coast para agentes containerizados, servidores internos vs servidores proxied pelo host.
- [Troubleshooting](TROUBLESHOOTING.md) — doctor, reinício do daemon, remoção de projeto e a opção nuclear de factory-reset.

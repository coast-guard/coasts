# Conceitos e Terminologia

Esta seção cobre os conceitos centrais e o vocabulário usados em todo o Coasts. Se você é novo no Coasts, comece aqui antes de mergulhar na configuração ou no uso avançado.

- [Coasts](COASTS.md) - runtimes autocontidos do seu projeto, cada um com suas próprias portas, volumes e atribuição de worktree.
- [Run](RUN.md) - criação de uma nova instância de Coast a partir da build mais recente, opcionalmente atribuindo uma worktree.
- [Remove](REMOVE.md) - remoção de uma instância de Coast e de seu estado de runtime isolado quando você precisa de uma recriação limpa ou quer desligar o Coasts.
- [Filesystem](FILESYSTEM.md) - a montagem compartilhada entre host e Coast, agentes no lado do host e alternância de worktree.
- [Private Paths](PRIVATE_PATHS.md) - isolamento por instância para caminhos do workspace que entram em conflito em bind mounts compartilhados.
- [Coast Daemon](DAEMON.md) - o plano de controle local `coastd` que executa operações de ciclo de vida.
- [Coast CLI](CLI.md) - a interface de terminal para comandos, scripts e fluxos de trabalho de agentes.
- [Coastguard](COASTGUARD.md) - a interface web iniciada com `coast ui` para observabilidade e controle.
- [Ports](PORTS.md) - portas canônicas vs portas dinâmicas e como o checkout alterna entre elas.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - links rápidos para o seu serviço principal, roteamento por subdomínio para isolamento de cookies e modelos de URL.
- [Assign and Unassign](ASSIGN.md) - alternância de uma Coast entre worktrees e as estratégias de atribuição disponíveis.
- [Checkout](CHECKOUT.md) - mapeamento de portas canônicas para uma instância de Coast e quando você precisa disso.
- [Lookup](LOOKUP.md) - descoberta de quais instâncias de Coast correspondem à worktree atual do agente.
- [Volume Topology](VOLUMES.md) - serviços compartilhados, volumes compartilhados, volumes isolados e snapshotting.
- [Shared Services](SHARED_SERVICES.md) - serviços de infraestrutura gerenciados pelo host e desambiguação de volumes.
- [Secrets and Extractors](SECRETS.md) - extração de segredos do host e injeção deles em contêineres Coast.
- [Builds](BUILDS.md) - a anatomia de uma build de coast, onde os artefatos ficam, limpeza automática e builds tipadas.
- [Coastfile Types](COASTFILE_TYPES.md) - variantes componíveis de Coastfile com extends, unset, omit e autostart.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) - o runtime DinD, a arquitetura Docker-in-Docker e como os serviços são executados dentro de uma Coast.
- [Bare Services](BARE_SERVICES.md) - execução de processos não conteinerizados dentro de uma Coast e por que você deveria conteinerizar em vez disso.
- [Bare Service Optimization](BARE_SERVICE_OPTIMIZATION.md) - instalações condicionais, cache, private_paths, conectividade com serviços compartilhados e estratégias de atribuição para bare services.
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - as variáveis `<SERVICE>_DYNAMIC_PORT` injetadas automaticamente e como usá-las em comandos de serviço.
- [Logs](LOGS.md) - leitura de logs de serviço de dentro de uma Coast, o tradeoff do MCP e o visualizador de logs do Coastguard.
- [Exec & Docker](EXEC_AND_DOCKER.md) - execução de comandos dentro de uma Coast e comunicação com o daemon Docker interno.
- [Agent Shells](AGENT_SHELLS.md) - TUIs de agentes conteinerizados, o tradeoff do OAuth e por que você provavelmente deveria executar agentes no host em vez disso.
- [MCP Servers](MCP_SERVERS.md) - configuração de ferramentas MCP dentro de uma Coast para agentes conteinerizados, servidores internos vs servidores com proxy pelo host.
- [Remotes](REMOTES.md) - execução de serviços em uma máquina remota via coast-service enquanto mantém o fluxo de trabalho local inalterado.
- [Troubleshooting](TROUBLESHOOTING.md) - doctor, reinicialização do daemon, remoção do projeto e a opção nuclear de reset de fábrica.

# Documentação do Coasts

## Instalando

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*Se você decidir não executar `coast daemon install`, você será responsável por iniciar o daemon manualmente com `coast daemon start` todas as vezes.*

## O que são Coasts?

Um Coast (**host containerizado**) é um runtime de desenvolvimento local. Coasts permitem que você execute múltiplos ambientes isolados para o mesmo projeto em uma única máquina.

Coasts são especialmente úteis para stacks complexas de `docker-compose` com muitos serviços interdependentes, mas também são igualmente eficazes para configurações locais de desenvolvimento não containerizadas. Coasts suportam uma ampla variedade de [padrões de configuração de runtime](concepts_and_terminology/RUNTIMES_AND_SERVICES.md) para que você possa moldar o ambiente ideal para múltiplos agentes trabalhando em paralelo.

Coasts são feitos para desenvolvimento local, não como um serviço em nuvem hospedado. Seus ambientes rodam localmente na sua máquina.

O projeto Coasts é gratuito, local, licenciado sob MIT, agnóstico ao provedor de agentes e agnóstico ao harness de agentes, sem upsells de IA.

Coasts funcionam com qualquer fluxo de trabalho de codificação agentiva que use worktrees. Nenhuma configuração especial do lado do harness é necessária.

## Por que Coasts para Worktrees

Git worktrees são excelentes para isolar alterações de código, mas por si só não resolvem o isolamento de runtime.

Quando você executa múltiplas worktrees em paralelo, você rapidamente esbarra em problemas de ergonomia:

- [Conflitos de porta](concepts_and_terminology/PORTS.md) entre serviços que esperam as mesmas portas do host.
- Configuração de banco de dados por worktree e [configuração de volumes](concepts_and_terminology/VOLUMES.md) que é tediosa de gerenciar.
- Ambientes de teste de integração que precisam de um encadeamento (wiring) de runtime personalizado por worktree.
- O inferno vivo de alternar worktrees e reconstruir o contexto de runtime a cada vez. Veja [Atribuir e Desatribuir](concepts_and_terminology/ASSIGN.md).

Se o Git é controle de versão para o seu código, Coasts são como o Git para os runtimes das suas worktrees.

Cada ambiente recebe suas próprias portas, então você pode inspecionar qualquer runtime de worktree em paralelo. Quando você [faz checkout](concepts_and_terminology/CHECKOUT.md) de um runtime de worktree, Coasts remapeiam esse runtime para as portas canônicas do seu projeto.

Coasts abstraem a configuração de runtime em uma camada modular simples por cima das worktrees, para que cada worktree possa rodar com o isolamento de que precisa sem manter manualmente uma configuração complexa por worktree.

## Requisitos

- macOS
- Docker Desktop
- Um projeto usando Git
- Node.js
- `socat` *(instalado com `curl -fsSL https://coasts.dev/install | sh` como uma dependência `depends_on` do Homebrew)*

```text
Nota sobre Linux: Ainda não testamos o Coasts no Linux, mas o suporte a Linux está planejado.
Você pode tentar executar o Coasts no Linux hoje, mas não fornecemos garantias de que ele funcionará corretamente.
```

## Containerizar Agentes?

Você pode containerizar um agente com um Coast. Isso pode parecer uma ótima ideia no começo, mas em muitos casos você na verdade não precisa executar seu agente de codificação dentro de um container.

Como Coasts compartilham o [filesystem](concepts_and_terminology/FILESYSTEM.md) com sua máquina host por meio de uma montagem de volume compartilhada, o fluxo de trabalho mais fácil e confiável é executar o agente no seu host e instruí-lo a executar tarefas pesadas de runtime (como testes de integração) dentro da instância do Coast usando [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md).

No entanto, se você quiser executar seu agente em um container, Coasts com certeza suportam isso via [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md). Você pode construir um rig incrivelmente intricado para essa configuração, incluindo [configuração de servidor MCP](concepts_and_terminology/MCP_SERVERS.md), mas isso pode não interoperar de forma limpa com o software de orquestração que existe hoje. Para a maioria dos fluxos de trabalho, agentes do lado do host são mais simples e mais confiáveis.

## Coasts vs Dev Containers

Coasts não são dev containers, e não são a mesma coisa.

Dev containers geralmente são projetados para montar uma IDE em um único workspace de desenvolvimento containerizado. Coasts são headless e otimizados como ambientes leves para uso paralelo por agentes com worktrees — múltiplos ambientes de runtime isolados e conscientes de worktree rodando lado a lado, com alternância rápida de checkout e controles de isolamento de runtime para cada instância.

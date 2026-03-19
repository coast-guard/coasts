# Documentação do Coasts

```youtube
MBGKSKau4sU
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## Instalação

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*Se você decidir não executar `coast daemon install`, você é responsável por iniciar o daemon manualmente com `coast daemon start` todas as vezes.*

## O que são Coasts?

Um Coast (**host conteinerizado**) é um runtime local de desenvolvimento. Coasts permitem que você execute múltiplos ambientes isolados para o mesmo projeto em uma única máquina.

Coasts são especialmente úteis para stacks complexas de `docker-compose` com muitos serviços interdependentes, mas são igualmente eficazes para configurações locais de desenvolvimento não conteinerizadas. Coasts suportam uma ampla variedade de [padrões de configuração de runtime](concepts_and_terminology/RUNTIMES_AND_SERVICES.md) para que você possa moldar o ambiente ideal para múltiplos agentes trabalhando em paralelo.

Coasts foram criados para desenvolvimento local, não como um serviço de nuvem hospedado. Seus ambientes rodam localmente na sua máquina.

O projeto Coasts é gratuito, local, licenciado sob MIT, agnóstico ao provedor de agentes e agnóstico ao harness de agentes, sem upsells de IA.

Coasts funcionam com qualquer workflow de codificação agentica que use worktrees. Nenhuma configuração especial do lado do harness é necessária.

## Por que Coasts para Worktrees

Worktrees do Git são excelentes para isolar mudanças de código, mas não resolvem o isolamento de runtime por si só.

Quando você executa múltiplas worktrees em paralelo, rapidamente encontra problemas de ergonomia:

- [Conflitos de porta](concepts_and_terminology/PORTS.md) entre serviços que esperam as mesmas portas do host.
- Configuração de banco de dados por worktree e [configuração de volumes](concepts_and_terminology/VOLUMES.md) que é tediosa de gerenciar.
- Ambientes de testes de integração que precisam de wiring de runtime personalizado por worktree.
- O inferno de alternar worktrees e reconstruir o contexto de runtime a cada vez. Veja [Assign and Unassign](concepts_and_terminology/ASSIGN.md).

Se o Git é controle de versão para o seu código, Coasts são como o Git para os runtimes das suas worktrees.

Cada ambiente recebe suas próprias portas, então você pode inspecionar qualquer runtime de worktree em paralelo. Quando você [faz checkout](concepts_and_terminology/CHECKOUT.md) de um runtime de worktree, Coasts remapeiam esse runtime para as portas canônicas do seu projeto.

Coasts abstraem a configuração de runtime em uma camada modular simples sobre as worktrees, para que cada worktree possa rodar com o isolamento de que precisa sem manter manualmente configurações complexas por worktree.

## Requisitos

- macOS ou Linux
- Docker Desktop no macOS, ou Docker Engine com o plugin Compose no Linux
- Um projeto usando Git
- Node.js
- `socat` (`brew install socat` no macOS, `sudo apt install socat` no Ubuntu)

```text
Linux note: Dynamic ports work out of the box on Linux.
If you need canonical ports below `1024`, see the checkout docs for the required host configuration.
```

## Agentes em Contêineres?

Você pode conteinerizar um agente com um Coast. Isso pode parecer uma ótima ideia no início, mas em muitos casos você não precisa realmente rodar seu agente de codificação dentro de um contêiner.

Como Coasts compartilham o [filesystem](concepts_and_terminology/FILESYSTEM.md) com sua máquina host por meio de um volume mount compartilhado, o workflow mais fácil e confiável é rodar o agente no seu host e instruí-lo a executar tarefas pesadas de runtime (como testes de integração) dentro da instância do Coast usando [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md).

No entanto, se você quiser rodar seu agente em um contêiner, Coasts suportam isso totalmente via [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md). Você pode construir um rig incrivelmente intricado para essa configuração, incluindo [configuração de servidor MCP](concepts_and_terminology/MCP_SERVERS.md), mas isso pode não interoperar de forma limpa com o software de orquestração que existe hoje. Para a maioria dos workflows, agentes no host são mais simples e mais confiáveis.

## Coasts vs Dev Containers

Coasts não são dev containers, e não são a mesma coisa.

Dev containers geralmente são projetados para montar uma IDE em um único workspace de desenvolvimento conteinerizado. Coasts são headless e otimizados como ambientes leves para uso paralelo de agentes com worktrees — múltiplos ambientes de runtime isolados, com consciência de worktree, rodando lado a lado, com alternância rápida de checkout e controles de isolamento de runtime para cada instância.

## Demo Repo

Se você quiser um pequeno projeto de exemplo para experimentar com Coasts, comece com o repositório [`coasts-demo`](https://github.com/coast-guard/coasts-demo).

## Coasts Video Course

Se você preferir vídeo, o [Coasts Video Course](learn-coasts-videos/README.md) cobre todos os conceitos centrais em menos de três minutos cada.

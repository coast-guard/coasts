# Habilidades para Agentes Host

Se você usa agentes de programação com IA no host enquanto seu app roda dentro do Coasts, seu agente normalmente precisa de duas partes de configuração específicas do Coast:

1. uma seção Coast Runtime sempre ativa no arquivo de instruções do projeto ou
   arquivo de regras do harness
2. uma habilidade reutilizável de fluxo de trabalho do Coast, como `/coasts`,
   quando o harness oferece suporte a habilidades de projeto

Sem a primeira parte, o agente edita arquivos mas esquece de usar `coast exec`.
Sem a segunda, toda atribuição de Coast, log e fluxo de UI precisa ser
reexplicado no chat.

Este guia mantém a configuração concreta e específica do Coast: qual arquivo
criar, que texto colocar nele e como isso muda de acordo com o harness.

## Por que os agentes precisam disso

Os Coasts compartilham o [sistema de arquivos](concepts_and_terminology/FILESYSTEM.md) entre
sua máquina host e o contêiner do Coast. Seu agente edita arquivos no host
e os serviços em execução dentro do Coast veem as mudanças imediatamente. Mas o
agente ainda precisa:

1. descobrir qual instância do Coast corresponde ao checkout atual
2. executar testes, builds e comandos de runtime dentro desse Coast
3. ler logs e status dos serviços do Coast
4. lidar com a atribuição de worktree com segurança quando nenhum Coast já está anexado

## O que vai em cada lugar

- `AGENTS.md`, `CLAUDE.md` ou `.cursor/rules/coast.md` — regras curtas do Coast
  que devem se aplicar em toda tarefa, mesmo que nenhuma skill seja invocada
- skill (`.agents/skills/...`, `.claude/skills/...` ou `.cursor/skills/...`)
  — o próprio fluxo de trabalho reutilizável do Coast, como `/coasts`
- arquivo de comando (`.claude/commands/...` ou `.cursor/commands/...`) —
  ponto de entrada explícito opcional para harnesses que oferecem suporte a
  isso; uma opção simples é fazer o comando reutilizar a skill

Se um repositório usa mais de um harness, mantenha a skill canônica do Coast em
um único lugar e exponha-a onde for necessário. Veja
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md).

## 1. Regras Coast Runtime sempre ativas

Adicione o bloco a seguir ao arquivo de instruções de projeto sempre ativo ou
arquivo de regras do harness (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/coast.md` ou equivalente):

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

Este bloco pertence ao arquivo sempre ativo porque as regras devem se aplicar
em toda tarefa, não apenas quando o agente entra explicitamente em um fluxo de
trabalho `/coasts`.

## 2. Skill reutilizável `/coasts`

Quando o harness oferece suporte a skills de projeto, salve o conteúdo da skill
como um `SKILL.md` no seu diretório de skills. O texto completo da skill está
em [skills_prompt.txt](skills_prompt.txt) (se estiver no modo CLI, use
`coast skills-prompt`) — tudo após o bloco Coast Runtime é o conteúdo da skill,
começando no frontmatter `---`.

Se você estiver usando superfícies específicas do Codex ou da OpenAI, também
pode adicionar opcionalmente `agents/openai.yaml` ao lado da skill para metadados
de exibição ou política de invocação. Esses metadados devem ficar ao lado da
skill, não substituí-la.

## Início rápido por harness

| Harness | Arquivo sempre ativo | Fluxo de trabalho reutilizável do Coast | Observações |
|---------|----------------------|------------------------------------------|-------------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Não há um arquivo de comando de projeto separado a recomendar para a documentação do Coast. Veja [Codex](harnesses/CODEX.md). |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md` é opcional, mas mantenha a lógica na skill. Veja [Claude Code](harnesses/CLAUDE_CODE.md). |
| Cursor | `AGENTS.md` ou `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` ou o compartilhado `.agents/skills/coasts/SKILL.md` | `.cursor/commands/coasts.md` é opcional. `.cursor/worktrees.json` é para bootstrap de worktree do Cursor, não política do Coast. Veja [Cursor](harnesses/CURSOR.md). |
| Conductor | `CLAUDE.md` | Comece com `CLAUDE.md`; use scripts e configurações do Conductor para comportamento específico do Conductor | Não suponha o comportamento completo de comandos de projeto do Claude Code. Se um novo comando não aparecer, feche e reabra completamente o Conductor. Veja [Conductor](harnesses/CONDUCTOR.md). |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | Esta é a superfície de harness mais limitada aqui. Use o layout no estilo Codex e não invente uma camada de comando nativa do T3 para a documentação do Coast. Veja [T3 Code](harnesses/T3_CODE.md). |

## Deixe o agente se configurar sozinho

A forma mais rápida é deixar o agente escrever os arquivos corretos por conta
própria. Copie o prompt abaixo para o chat do seu agente — ele inclui o bloco
Coast Runtime, o bloco de skill `coasts` e instruções específicas por harness
sobre onde cada parte deve ficar.

```prompt-copy
skills_prompt.txt
```

Você também pode obter a mesma saída pela CLI executando `coast skills-prompt`.

## Configuração manual

- **Codex:** coloque a seção Coast Runtime em `AGENTS.md`, depois coloque a
  skill reutilizável `coasts` em `.agents/skills/coasts/SKILL.md`.
- **Claude Code:** coloque a seção Coast Runtime em `CLAUDE.md`, depois coloque a
  skill reutilizável `coasts` em `.claude/skills/coasts/SKILL.md`. Só adicione
  `.claude/commands/coasts.md` se você quiser especificamente um arquivo de comando.
- **Cursor:** coloque a seção Coast Runtime em `AGENTS.md` se você quiser as
  instruções mais portáveis, ou em `.cursor/rules/coast.md` se quiser uma
  regra de projeto nativa do Cursor. Coloque o fluxo de trabalho reutilizável
  `coasts` em `.cursor/skills/coasts/SKILL.md` para um repositório só de Cursor,
  ou em `.agents/skills/coasts/SKILL.md` se o repositório for compartilhado com
  outros harnesses. Só adicione `.cursor/commands/coasts.md` se você quiser
  especificamente um arquivo de comando explícito.
- **Conductor:** coloque a seção Coast Runtime em `CLAUDE.md`. Use os scripts
  de Repository Settings do Conductor para bootstrap ou comportamento de execução
  específicos do Conductor. Se você adicionar um comando e ele não aparecer,
  feche e reabra totalmente o app.
- **T3 Code:** use o mesmo layout do Codex: `AGENTS.md` mais
  `.agents/skills/coasts/SKILL.md`. Trate o T3 Code aqui como um harness fino
  no estilo Codex, não como uma superfície separada de comandos do Coast.
- **Multiple harnesses:** mantenha a skill canônica em
  `.agents/skills/coasts/SKILL.md`. O Cursor pode carregar isso diretamente;
  exponha para o Claude Code por meio de `.claude/skills/coasts/` se necessário.

## Leitura adicional

- Leia o [guia de Harnesses](harnesses/README.md) para a matriz por harness
- Leia [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md) para o padrão de
  layout compartilhado
- Leia a [documentação dos Coastfiles](coastfiles/README.md) para aprender o esquema completo
  de configuração
- Aprenda os comandos da [CLI do Coast](concepts_and_terminology/CLI.md) para gerenciar
  instâncias
- Explore o [Coastguard](concepts_and_terminology/COASTGUARD.md), a interface web para
  observar e controlar seus Coasts

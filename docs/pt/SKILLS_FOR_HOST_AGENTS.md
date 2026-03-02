# Habilidades para Agentes no Host

Se você estiver usando agentes de codificação com IA (Claude Code, Codex, Conductor, Cursor ou similares) em um projeto que usa Coasts, seu agente precisa de uma habilidade que o ensine a interagir com o runtime do Coast. Sem isso, o agente vai editar arquivos, mas não saberá como executar testes, verificar logs ou confirmar que suas alterações funcionam dentro do ambiente em execução.

Este guia mostra como configurar essa habilidade.

## Por que os Agentes Precisam Disso

Os Coasts compartilham o [filesystem](concepts_and_terminology/FILESYSTEM.md) entre sua máquina host e o container do Coast. Seu agente edita arquivos no host e os serviços em execução dentro do Coast veem as mudanças imediatamente. Mas o agente ainda precisa:

1. **Descobrir com qual instância do Coast ele está trabalhando** — `coast lookup` resolve isso a partir do diretório atual do agente.
2. **Executar comandos dentro do Coast** — testes, builds e outras tarefas de runtime acontecem dentro do container via `coast exec`.
3. **Ler logs e verificar o status dos serviços** — `coast logs` e `coast ps` dão ao agente feedback do runtime.

A habilidade abaixo ensina ao agente as três coisas.

## A Habilidade

Adicione o seguinte à habilidade, regras ou arquivo de prompt existente do seu agente. Se o seu agente já tiver instruções para executar testes ou interagir com seu ambiente de dev, isto deve ficar junto dessas instruções — ele ensina o agente a usar Coasts para operações de runtime.

```text-copy
This project uses Coasts (containerized host) for isolated development environments.
Your code edits are automatically visible inside the running Coast — the filesystem
is shared between the host and the container.

=== ORIENTATION ===

Before running any runtime commands, discover which Coast instance matches your
current working directory:

  coast lookup

This prints the instance name, ports, URLs, and example commands. Use the instance
name from the output for all subsequent commands.

If you need deeper context on how Coasts work, read these docs:

  coast docs --path concepts_and_terminology/LOOKUP.md
  coast docs --path concepts_and_terminology/FILESYSTEM.md
  coast docs --path concepts_and_terminology/EXEC_AND_DOCKER.md
  coast docs --path concepts_and_terminology/LOGS.md

=== RUNNING COMMANDS ===

Use `coast exec` to run commands inside the Coast. The shell starts at the workspace
root (where the Coastfile is). cd to your target directory first:

  coast exec <instance> -- sh -c "cd <dir> && <command>"

Examples:

  coast exec dev-1 -- sh -c "cd src && npm test"
  coast exec dev-1 -- sh -c "cd backend && go test ./..."
  coast exec dev-1 -- sh -c "cd apps/web && npx playwright test"

=== RUNTIME FEEDBACK ===

Check service status:

  coast ps <instance>

Read service logs:

  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

=== TROUBLESHOOTING ===

If you encounter errors or unfamiliar behavior, search the Coast docs:

  coast search-docs "error message or description"

This uses semantic search — describe the problem in natural language and it will
find the relevant documentation.

=== RULES ===

- Always run `coast lookup` before your first runtime command in a session.
- Do not run services directly on the host. Use `coast exec` for all runtime tasks.
- File edits on the host are instantly visible inside the Coast. You do not need
  to copy files or rebuild after editing.
- If `coast lookup` returns no instances, the Coast may not be running. Suggest
  `coast run dev-1` or check `coast ls` for the project state.
```

## Adicionando a Habilidade ao Seu Agente

A forma mais rápida é deixar o agente se configurar sozinho. Execute um destes a partir do diretório do seu projeto:

```sh
# Claude Code
claude -p "$(coast skills-prompt)"

# Codex
codex "$(coast skills-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast skills-prompt)"
```

Isso fornece ao agente o texto da habilidade e instruções para gravá-lo no próprio arquivo de configuração (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/coast.md`, etc.).

### Configuração manual

Se você preferir adicionar a habilidade por conta própria:

- **Claude Code:** Adicione o texto da habilidade ao arquivo `CLAUDE.md` do seu projeto.
- **Codex:** Adicione o texto da habilidade ao arquivo `AGENTS.md` do seu projeto.
- **Cursor:** Crie `.cursor/rules/coast.md` na raiz do seu projeto e cole o texto da habilidade.
- **Outros agentes:** Cole o texto da habilidade em qualquer arquivo de prompt ou regras em nível de projeto que seu agente leia ao iniciar.

## Leitura adicional

- Leia a [documentação de Coastfiles](coastfiles/README.md) para aprender o esquema completo de configuração
- Aprenda os comandos da [CLI do Coast](concepts_and_terminology/CLI.md) para gerenciar instâncias
- Explore o [Coastguard](concepts_and_terminology/COASTGUARD.md), a UI web para observar e controlar seus Coasts
- Navegue por [Conceitos & Terminologia](concepts_and_terminology/README.md) para ter a visão completa de como os Coasts funcionam

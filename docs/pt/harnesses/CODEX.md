# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) cria worktrees em `$CODEX_HOME/worktrees` (normalmente `~/.codex/worktrees`). Cada worktree fica sob um diretório de hash opaco como `~/.codex/worktrees/a0db/project-name`, começa em um HEAD destacado e é limpo automaticamente com base na política de retenção do Codex.

Da [documentação do Codex](https://developers.openai.com/codex/app/worktrees/):

> Posso controlar onde os worktrees são criados?
> Ainda não. O Codex cria worktrees em `$CODEX_HOME/worktrees` para que possa gerenciá-los de forma consistente.

Como esses worktrees ficam fora da raiz do projeto, o Coasts precisa de
configuração explícita para descobri-los e montá-los.

## Setup

Adicione `~/.codex/worktrees` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

O Coasts expande `~` em tempo de execução e trata qualquer caminho que comece com `~/` ou `/` como
externo. Veja [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para
detalhes.

Após alterar `worktree_dir`, as instâncias existentes devem ser **recriadas** para que o bind mount entre em vigor:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coasts lê o novo Coastfile), mas
atribuir a um worktree do Codex requer o bind mount dentro do contêiner.

## Onde vai a orientação do Coasts

Use o arquivo de instruções de projeto do Codex e o layout compartilhado de skill para trabalhar com
Coasts:

- coloque as regras curtas do Coast Runtime em `AGENTS.md`
- coloque o fluxo de trabalho reutilizável `/coasts` em `.agents/skills/coasts/SKILL.md`
- o Codex expõe essa skill como o comando `/coasts`
- se você usar metadados específicos do Codex, mantenha-os ao lado da skill em
  `.agents/skills/coasts/agents/openai.yaml`
- não crie um arquivo de comando de projeto separado apenas para documentação sobre Coasts; a
  skill é a superfície reutilizável
- se este repositório também usar Cursor ou Claude Code, mantenha a skill canônica em
  `.agents/skills/` e exponha-a a partir daí. Veja
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) e
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

Por exemplo, um `.agents/skills/coasts/agents/openai.yaml` mínimo poderia ser
assim:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

Isso mantém a skill visível no Codex com um rótulo melhor e torna `/coasts` um
comando explícito. Só adicione `dependencies.tools` se a skill também precisar de servidores MCP
ou outro encadeamento de ferramentas gerenciado pela OpenAI.

## O que o Coasts faz

- **Run** -- `coast run <name>` cria uma nova instância do Coast a partir do build mais recente. Use `coast run <name> -w <worktree>` para criar e atribuir um worktree do Codex em uma única etapa. Veja [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- Na criação do contêiner, o Coasts monta
  `~/.codex/worktrees` no contêiner em `/host-external-wt/{index}`.
- **Descoberta** -- `git worktree list --porcelain` tem escopo de repositório, então apenas os worktrees do Codex pertencentes ao projeto atual aparecem, mesmo que o diretório contenha worktrees de muitos projetos.
- **Nomenclatura** -- Worktrees com HEAD destacado aparecem como seu caminho relativo dentro do diretório externo (`a0db/my-app`, `eca7/my-app`). Worktrees baseados em branch mostram o nome da branch.
- **Atribuição** -- `coast assign` remonta `/workspace` a partir do caminho do bind mount externo.
- **Sincronização de arquivos ignorados pelo Git** -- É executada no sistema de arquivos do host com caminhos absolutos, funciona sem o bind mount.
- **Detecção de órfãos** -- O observador do git varre diretórios externos
  recursivamente, filtrando por ponteiros gitdir em `.git`. Se o Codex excluir um
  worktree, o Coasts remove automaticamente a atribuição da instância.

## Exemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.claude/worktrees/` -- Claude Code (local, sem tratamento especial)
- `~/.codex/worktrees/` -- Codex (externo, com bind mount)

## Limitações

- O Codex pode limpar worktrees a qualquer momento. A detecção de órfãos no Coasts
  lida com isso de forma elegante.

# Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) cria
worktrees dentro do projeto em `.claude/worktrees/`. Como esse diretório
fica dentro do repositório, o Coasts pode descobrir e atribuir worktrees do
Claude Code sem qualquer bind mount externo.

O Claude Code também é o harness aqui com a separação mais clara entre três
camadas para o Coasts:

- `CLAUDE.md` para regras curtas, sempre ativas, para trabalhar com o Coasts
- `.claude/skills/coasts/SKILL.md` para o fluxo reutilizável `/coasts`
- `.claude/commands/coasts.md` somente quando você quiser um arquivo de comando como um ponto de entrada
  extra

## Setup

Adicione `.claude/worktrees` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

Como `.claude/worktrees` é relativo ao projeto, nenhum bind mount externo é
necessário.

## Onde a orientação do Coasts vai

### `CLAUDE.md`

Coloque aqui as regras para o Coasts que devem se aplicar em toda tarefa. Mantenha isto curto e
operacional:

- execute `coast lookup` antes do primeiro comando de runtime em uma sessão
- use `coast exec` para testes, builds e comandos de serviço
- use `coast ps` e `coast logs` para feedback de runtime
- pergunte antes de criar ou reatribuir um Coast quando não existir correspondência

### `.claude/skills/coasts/SKILL.md`

Coloque aqui o fluxo reutilizável `/coasts`. Este é o lugar certo para um fluxo
que:

1. executa `coast lookup` e reutiliza o Coast correspondente
2. recorre a `coast ls` quando não há correspondência
3. oferece `coast run`, `coast assign`, `coast unassign`, `coast checkout` e
   `coast ui`
4. usa diretamente a CLI do Coast como contrato em vez de encapsulá-la

Se este repositório também usa Codex, T3 Code ou Cursor, veja
[Multiple Harnesses](MULTIPLE_HARNESSES.md) e mantenha a skill canônica em
`.agents/skills/coasts/`, depois exponha-a ao Claude Code.

### `.claude/commands/coasts.md`

O Claude Code também oferece suporte a arquivos de comando do projeto. Para a documentação sobre Coasts, trate
isto como opcional:

- use isso somente quando você quiser especificamente um arquivo de comando
- uma opção simples é fazer o comando reutilizar a mesma skill
- se você der ao comando suas próprias instruções separadas, estará assumindo uma
  segunda cópia do fluxo para manter

## Exemplo de estrutura

### Apenas Claude Code

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

Se este repositório também usa Codex, T3 Code ou Cursor, use o padrão compartilhado em
[Multiple Harnesses](MULTIPLE_HARNESSES.md) em vez de duplicá-lo aqui,
porque orientações específicas de provedor duplicadas ficam mais difíceis de manter em sincronia a cada
vez que você adiciona outro harness.

## O que o Coasts faz

- **Executar** — `coast run <name>` cria uma nova instância Coast a partir da build mais recente. Use `coast run <name> -w <worktree>` para criar e atribuir uma worktree do Claude Code em uma etapa. Veja [Run](../concepts_and_terminology/RUN.md).
- **Descoberta** — O Coasts lê `.claude/worktrees` como qualquer outro diretório local de
  worktree.
- **Nomenclatura** — As worktrees do Claude Code seguem o mesmo comportamento de nomenclatura
  de worktree local que outras worktrees dentro do repositório na UI e na CLI do Coasts.
- **Atribuir** — `coast assign` pode alternar `/workspace` para uma worktree do Claude Code
  sem qualquer indireção de bind-mount externo.
- **Sincronização de itens ignorados pelo Git** — Funciona normalmente porque as worktrees vivem dentro da
  árvore do repositório.
- **Detecção de órfãos** — Se o Claude Code remover uma worktree, o Coasts pode detectar
  o gitdir ausente e desatribui-la quando necessário.

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

- `.claude/worktrees/` — worktrees do Claude Code
- `~/.codex/worktrees/` — worktrees do Codex se você também usar Codex neste repositório

## Limitações

- Se você duplicar o mesmo fluxo `/coasts` em `CLAUDE.md`,
  `.claude/skills` e `.claude/commands`, essas cópias entrarão em divergência. Mantenha
  `CLAUDE.md` curto e mantenha o fluxo reutilizável em uma única skill.
- Se você quiser que um repositório funcione bem em múltiplos harnesses, prefira o padrão compartilhado
  em [Multiple Harnesses](MULTIPLE_HARNESSES.md).

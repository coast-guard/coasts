# Cursor

[Cursor](https://cursor.com/docs/agent/overview) pode trabalhar diretamente no seu
checkout atual, e seu recurso de Parallel Agents também pode criar git
worktrees em `~/.cursor/worktrees/<project-name>/`.

Para a documentação sobre Coasts, isso significa que há dois casos de configuração:

- se você estiver apenas usando o Cursor no checkout atual, nenhuma entrada
  `worktree_dir` específica do Cursor é necessária
- se você usar Cursor Parallel Agents, adicione o diretório de worktree do
  Cursor a `worktree_dir` para que o Coasts possa descobrir e atribuir esses worktrees

## Configuração

### Apenas checkout atual

Se o Cursor estiver apenas editando o checkout que você já abriu, o Coasts não precisa
de nenhum caminho de worktree específico do Cursor. O Coasts tratará esse checkout como
qualquer outra raiz de repositório local.

### Cursor Parallel Agents

Se você usa Parallel Agents, adicione `~/.cursor/worktrees/<project-name>` a
`worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

O Cursor armazena cada worktree de agente sob esse diretório por projeto. O Coasts
expande `~` em tempo de execução e trata o caminho como externo, então instâncias
existentes devem ser recriadas para que o bind mount tenha efeito:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente após a alteração no Coastfile, mas
atribuir a um worktree do Cursor Parallel Agent requer o bind mount externo
dentro do contêiner.

## Onde a orientação do Coasts deve ficar

### `AGENTS.md` ou `.cursor/rules/coast.md`

Coloque aqui as regras curtas e sempre ativas do Coast Runtime:

- use `AGENTS.md` se quiser as instruções de projeto mais portáveis
- use `.cursor/rules/coast.md` se quiser regras de projeto nativas do Cursor e
  suporte à UI de configurações
- não duplique o mesmo bloco do Coast Runtime em ambos, a menos que tenha um motivo
  claro

### `.cursor/skills/coasts/SKILL.md` ou `.agents/skills/coasts/SKILL.md` compartilhado

Coloque aqui o fluxo de trabalho reutilizável de `/coasts`:

- para um repositório somente Cursor, `.cursor/skills/coasts/SKILL.md` é um local natural
- para um repositório com múltiplos harnesses, mantenha a skill canônica em
  `.agents/skills/coasts/SKILL.md`; o Cursor pode carregá-la diretamente
- a skill deve ser dona do fluxo de trabalho real de `/coasts`: `coast lookup`,
  `coast ls`, `coast run`, `coast assign`, `coast unassign`,
  `coast checkout` e `coast ui`

### `.cursor/commands/coasts.md`

O Cursor também oferece suporte a comandos de projeto. Para a documentação sobre Coasts,
trate os comandos como opcionais:

- adicione um comando apenas quando quiser um ponto de entrada explícito para `/coasts`
- uma opção simples é fazer o comando reutilizar a mesma skill
- se você der ao comando suas próprias instruções separadas, estará assumindo a manutenção
  de uma segunda cópia do fluxo de trabalho

### `.cursor/worktrees.json`

Use `.cursor/worktrees.json` para o bootstrap de worktree do próprio Cursor, não para
a política do Coasts:

- instalar dependências
- copiar ou criar symlinks de arquivos `.env`
- executar migrações de banco de dados ou outras etapas de bootstrap únicas

Não mova as regras do Coast Runtime nem o fluxo de trabalho da Coast CLI para
`.cursor/worktrees.json`.

## Exemplo de layout

### Somente Cursor

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # opcional
.cursor/rules/coast.md            # alternativa opcional ao AGENTS.md
.cursor/worktrees.json            # opcional, para bootstrap de Parallel Agents
```

### Cursor mais outros harnesses

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # opcional
```

## O que o Coasts faz

- **Executar** — `coast run <name>` cria uma nova instância do Coast a partir da build mais recente. Use `coast run <name> -w <worktree>` para criar e atribuir um worktree do Cursor em uma única etapa. Veja [Run](../concepts_and_terminology/RUN.md).
- **Checkout atual** — Nenhum tratamento especial do Cursor é necessário quando o Cursor está
  trabalhando diretamente no repositório que você abriu.
- **Bind mount** — Para Parallel Agents, o Coasts monta
  `~/.cursor/worktrees/<project-name>` no contêiner em
  `/host-external-wt/{index}`.
- **Descoberta** — `git worktree list --porcelain` continua com escopo de repositório, então o Coasts
  mostra apenas worktrees do Cursor que pertencem ao projeto atual.
- **Nomenclatura** — Os worktrees do Cursor Parallel Agent aparecem por seus nomes de branch no
  CLI e na UI do Coasts.
- **Atribuir** — `coast assign` remonta `/workspace` a partir do caminho de bind mount
  externo quando um worktree do Cursor é selecionado.
- **Sincronização de arquivos ignorados pelo Git** — Continua funcionando no sistema de arquivos do host com caminhos
  absolutos.
- **Detecção de órfãos** — Se o Cursor limpar worktrees antigos, o Coasts pode detectar
  o gitdir ausente e desatribuí-los quando necessário.

## Exemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.cursor/worktrees/my-app"]
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
- `~/.codex/worktrees/` — worktrees do Codex
- `~/.cursor/worktrees/my-app/` — worktrees do Cursor Parallel Agent

## Limitações

- Se você não estiver usando Cursor Parallel Agents, não adicione
  `~/.cursor/worktrees/<project-name>` só porque por acaso está editando no
  Cursor.
- Mantenha as regras do Coast Runtime em um único lugar sempre ativo: `AGENTS.md` ou
  `.cursor/rules/coast.md`. Duplicar ambos convida à divergência.
- Mantenha o fluxo de trabalho reutilizável de `/coasts` em uma skill. `.cursor/worktrees.json` é
  para bootstrap do Cursor, não política do Coasts.
- Se um repositório for compartilhado entre Cursor, Codex, Claude Code ou T3 Code, prefira
  o layout compartilhado em [Multiple Harnesses](MULTIPLE_HARNESSES.md).

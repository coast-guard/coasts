# Conductor

[Conductor](https://conductor.build/) executa agentes Claude Code em paralelo, cada um em seu próprio workspace isolado. Workspaces são git worktrees armazenados em `~/conductor/workspaces/<project-name>/`. Cada workspace é feito checkout em uma branch nomeada.

Como esses worktrees ficam fora da raiz do projeto, o Coasts precisa de
configuração explícita para descobri-los e montá-los.

## Configuração

Adicione `~/conductor/workspaces/<project-name>` a `worktree_dir`. Diferentemente do Codex (que armazena todos os projetos em um único diretório plano), o Conductor aninha worktrees em um subdiretório por projeto, então o caminho deve incluir o nome do projeto. No exemplo abaixo, `my-app` deve corresponder ao nome real da pasta em `~/conductor/workspaces/` para o seu repositório.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

O Conductor permite configurar o caminho dos workspaces por repositório, então o padrão `~/conductor/workspaces` pode não corresponder à sua configuração. Verifique as configurações do repositório no Conductor para encontrar o caminho real e ajuste conforme necessário — o princípio é o mesmo independentemente de onde o diretório esteja.

O Coasts expande `~` em tempo de execução e trata qualquer caminho que comece com `~/` ou `/` como
externo. Consulte [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para
detalhes.

Após alterar `worktree_dir`, instâncias existentes devem ser **recriadas** para que o bind mount entre em vigor:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coasts lê o novo Coastfile), mas
atribuir a um worktree do Conductor requer o bind mount dentro do container.

## Onde vai a orientação do Coasts

Trate o Conductor como seu próprio harness para trabalhar com Coasts:

- coloque as regras curtas do Coast Runtime em `CLAUDE.md`
- use scripts de Configurações de Repositório do Conductor para comportamento de setup ou execução que seja
  realmente específico do Conductor
- não presuma aqui o comportamento completo de comandos de projeto ou project skills do Claude Code
- se você adicionar um comando e ele não aparecer, feche e reabra completamente o
  Conductor antes de testar novamente
- se este repositório também usar outros harnesses, consulte
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) e
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) para maneiras de manter o
  fluxo de trabalho compartilhado de `/coasts` em um só lugar

## O que o Coasts faz

- **Run** — `coast run <name>` cria uma nova instância do Coast a partir da build mais recente. Use `coast run <name> -w <worktree>` para criar e atribuir um worktree do Conductor em uma única etapa. Veja [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — Na criação do container, o Coasts monta
  `~/conductor/workspaces/<project-name>` dentro do container em
  `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` tem escopo de repositório, então apenas worktrees pertencentes ao projeto atual aparecem.
- **Naming** — Worktrees do Conductor usam branches nomeadas, então aparecem pelo nome da branch
  na UI e CLI do Coasts (por exemplo, `scroll-to-bottom-btn`). Uma branch só pode
  estar em checkout em um workspace do Conductor por vez.
- **Assign** — `coast assign` remonta `/workspace` a partir do caminho do bind mount externo.
- **Gitignored sync** — É executado no sistema de arquivos do host com caminhos absolutos e funciona sem o bind mount.
- **Orphan detection** — O watcher do git varre diretórios externos
  recursivamente, filtrando por ponteiros `.git` gitdir. Se o Conductor arquivar ou
  excluir um workspace, o Coasts remove automaticamente a atribuição da instância.

## Exemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = ["~/conductor/workspaces/my-app"]
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

- `~/conductor/workspaces/my-app/` — Conductor (externo, com bind mount; substitua `my-app` pelo nome da pasta do seu repositório)

## Variáveis de Ambiente do Conductor

- Evite depender de variáveis de ambiente específicas do Conductor (por exemplo,
  `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) para configuração em tempo de execução
  dentro do Coasts. O Coasts gerencia portas, caminhos de workspace e descoberta de serviços
  de forma independente — use Coastfile `[ports]` e `coast exec` em vez disso.

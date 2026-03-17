# Conductor

[Conductor](https://conductor.build/) executa agentes Claude Code em paralelo, cada um em seu próprio workspace isolado. Workspaces são git worktrees armazenados em `~/conductor/workspaces/<project-name>/`. Cada workspace é feito checkout em uma branch nomeada.

Como esses worktrees ficam fora da raiz do projeto, o Coast precisa de configuração explícita para descobri-los e montá-los.

## Configuração

Adicione `~/conductor/workspaces/<project-name>` a `worktree_dir`. Diferentemente do Codex (que armazena todos os projetos em um único diretório plano), o Conductor aninha worktrees em um subdiretório por projeto, então o caminho deve incluir o nome do projeto. No exemplo abaixo, `my-app` deve corresponder ao nome real da pasta em `~/conductor/workspaces/` para o seu repositório.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

O Conductor permite configurar o caminho dos workspaces por repositório, então o padrão `~/conductor/workspaces` pode não corresponder à sua configuração. Verifique as configurações do repositório no Conductor para encontrar o caminho real e ajuste conforme necessário — o princípio é o mesmo independentemente de onde o diretório esteja.

O Coast expande `~` em tempo de execução e trata qualquer caminho que comece com `~/` ou `/` como externo. Consulte [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para detalhes.

Após alterar `worktree_dir`, instâncias existentes devem ser **recriadas** para que o bind mount entre em vigor:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coast lê o novo Coastfile), mas atribuir a um worktree do Conductor requer o bind mount dentro do container.

## O que o Coast faz

- **Bind mount** — Na criação do container, o Coast monta `~/conductor/workspaces/<project-name>` dentro do container em `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` tem escopo de repositório, então apenas worktrees pertencentes ao projeto atual aparecem.
- **Naming** — Worktrees do Conductor usam branches nomeadas, então aparecem pelo nome da branch na UI e CLI do Coast (por exemplo, `scroll-to-bottom-btn`). Uma branch só pode estar em checkout em um workspace do Conductor por vez.
- **Assign** — `coast assign` remonta `/workspace` a partir do caminho do bind mount externo.
- **Gitignored sync** — É executado no sistema de arquivos do host com caminhos absolutos e funciona sem o bind mount.
- **Orphan detection** — O watcher do git varre diretórios externos recursivamente, filtrando por ponteiros `.git` gitdir. Se o Conductor arquivar ou excluir um workspace, o Coast remove automaticamente a atribuição da instância.

## Exemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/conductor/workspaces/my-app"]
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

- `.worktrees/` — Worktrees gerenciados pelo Coast
- `.claude/worktrees/` — Claude Code (local, sem tratamento especial)
- `~/.codex/worktrees/` — Codex (externo, com bind mount)
- `~/conductor/workspaces/my-app/` — Conductor (externo, com bind mount; substitua `my-app` pelo nome da pasta do seu repositório)

## Variáveis de Ambiente do Conductor

- Evite depender de variáveis de ambiente específicas do Conductor (por exemplo, `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) para configuração em tempo de execução dentro de Coasts. O Coast gerencia portas, caminhos de workspace e descoberta de serviços de forma independente — use Coastfile `[ports]` e `coast exec` em vez disso.

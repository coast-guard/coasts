# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) é um harness de agente de programação de código aberto da Ping. Cada workspace é uma git worktree armazenada em `~/.t3/worktrees/<project-name>/`, com checkout em uma branch nomeada.

Como essas worktrees ficam fora da raiz do projeto, o Coast precisa de configuração explícita para descobri-las e montá-las.

## Configuração

Adicione `~/.t3/worktrees/<project-name>` a `worktree_dir`. O T3 Code aninha worktrees em um subdiretório por projeto, então o caminho deve incluir o nome do projeto. No exemplo abaixo, `my-app` deve corresponder ao nome real da pasta em `~/.t3/worktrees/` para o seu repositório.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

O Coast expande `~` em tempo de execução e trata qualquer caminho que comece com `~/` ou `/` como externo. Veja [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para detalhes.

Após alterar `worktree_dir`, instâncias existentes devem ser **recriadas** para que o bind mount tenha efeito:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coast lê o novo Coastfile), mas atribuir a uma worktree do T3 Code requer o bind mount dentro do container.

## O que o Coast faz

- **Bind mount** — Na criação do container, o Coast monta `~/.t3/worktrees/<project-name>` no container em `/host-external-wt/{index}`.
- **Descoberta** — `git worktree list --porcelain` é delimitado ao repositório, então apenas worktrees pertencentes ao projeto atual aparecem.
- **Nomenclatura** — As worktrees do T3 Code usam branches nomeadas, então aparecem pelo nome da branch na UI e CLI do Coast.
- **Atribuição** — `coast assign` remonta `/workspace` a partir do caminho de bind mount externo.
- **Sincronização de arquivos ignorados pelo Git** — É executada no sistema de arquivos do host com caminhos absolutos, funciona sem o bind mount.
- **Detecção de órfãos** — O watcher do git varre diretórios externos recursivamente, filtrando por ponteiros gitdir de `.git`. Se o T3 Code remover um workspace, o Coast remove automaticamente a atribuição da instância.

## Exemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
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

- `.worktrees/` — Worktrees gerenciadas pelo Coast
- `.claude/worktrees/` — Claude Code (local, sem tratamento especial)
- `~/.codex/worktrees/` — Codex (externo, montado com bind mount)
- `~/.t3/worktrees/my-app/` — T3 Code (externo, montado com bind mount; substitua `my-app` pelo nome da pasta do seu repositório)

## Limitações

- O Coast descobre e monta worktrees do T3 Code, mas não as cria nem as exclui.
- Novas worktrees criadas por `coast assign` sempre vão para o `default_worktree_dir` local, nunca para um diretório externo.
- Evite depender de variáveis de ambiente específicas do T3 Code para configuração de runtime dentro de Coasts. O Coast gerencia portas, caminhos de workspace e descoberta de serviços de forma independente — use Coastfile `[ports]` e `coast exec` em vez disso.

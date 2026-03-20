# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) cria git worktrees em
`~/.t3/worktrees/<project-name>/`, com checkout em branches nomeadas.

No T3 Code, coloque as regras sempre ativas do Coast Runtime em `AGENTS.md` e o
workflow reutilizável `/coasts` em `.agents/skills/coasts/SKILL.md`.

Como essas worktrees ficam fora da raiz do projeto, o Coasts precisa de
configuração explícita para descobri-las e montá-las.

## Configuração

Adicione `~/.t3/worktrees/<project-name>` a `worktree_dir`. O T3 Code aninha worktrees em um subdiretório por projeto, então o caminho deve incluir o nome do projeto. No exemplo abaixo, `my-app` deve corresponder ao nome real da pasta em `~/.t3/worktrees/` para o seu repositório.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

O Coasts expande `~` em tempo de execução e trata qualquer caminho que comece com `~/` ou `/` como externo. Veja [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para detalhes.

Após alterar `worktree_dir`, instâncias existentes devem ser **recriadas** para que o bind mount tenha efeito:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coasts lê o novo Coastfile), mas
atribuir a uma worktree do T3 Code requer o bind mount dentro do container.

## Onde vai a orientação do Coasts

Use este layout para T3 Code:

- coloque as regras curtas do Coast Runtime em `AGENTS.md`
- coloque o workflow reutilizável `/coasts` em `.agents/skills/coasts/SKILL.md`
- não adicione uma camada separada de comando de projeto ou slash-command específica do T3 para
  Coasts
- se este repositório usar múltiplos harnesses, veja
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) e
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

## O que o Coasts faz

- **Run** — `coast run <name>` cria uma nova instância do Coast a partir do build mais recente. Use `coast run <name> -w <worktree>` para criar e atribuir uma worktree do T3 Code em uma única etapa. Veja [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — Na criação do container, o Coasts monta
  `~/.t3/worktrees/<project-name>` no container em
  `/host-external-wt/{index}`.
- **Descoberta** — `git worktree list --porcelain` é delimitado ao repositório, então apenas worktrees pertencentes ao projeto atual aparecem.
- **Nomenclatura** — As worktrees do T3 Code usam branches nomeadas, então aparecem pelo nome da branch na UI e CLI do Coasts.
- **Atribuição** — `coast assign` remonta `/workspace` a partir do caminho de bind mount externo.
- **Sincronização de arquivos ignorados pelo Git** — É executada no sistema de arquivos do host com caminhos absolutos, funciona sem o bind mount.
- **Detecção de órfãos** — O watcher do git varre diretórios externos
  recursivamente, filtrando por ponteiros gitdir de `.git`. Se o T3 Code remover um
  workspace, o Coasts remove automaticamente a atribuição da instância.

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

- `.claude/worktrees/` — Claude Code (local, sem tratamento especial)
- `~/.codex/worktrees/` — Codex (externo, montado com bind mount)
- `~/.t3/worktrees/my-app/` — T3 Code (externo, montado com bind mount; substitua `my-app` pelo nome da pasta do seu repositório)

## Limitações

- Evite depender de variáveis de ambiente específicas do T3 Code para configuração de runtime dentro do Coasts. O Coasts gerencia portas, caminhos de workspace e descoberta de serviços de forma independente — use Coastfile `[ports]` e `coast exec` em vez disso.

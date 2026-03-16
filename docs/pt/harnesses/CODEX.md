# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) cria worktrees em `$CODEX_HOME/worktrees` (normalmente `~/.codex/worktrees`). Cada worktree fica sob um diretório de hash opaco como `~/.codex/worktrees/a0db/project-name`, começa em um HEAD destacado e é limpo automaticamente com base na política de retenção do Codex.

Da [documentação do Codex](https://developers.openai.com/codex/app/worktrees/):

> Posso controlar onde os worktrees são criados?
> Ainda não. O Codex cria worktrees em `$CODEX_HOME/worktrees` para que possa gerenciá-los de forma consistente.

Como esses worktrees ficam fora da raiz do projeto, o Coast precisa de configuração explícita para descobri-los e montá-los.

## Setup

Adicione `~/.codex/worktrees` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

O Coast expande `~` em tempo de execução e trata qualquer caminho que comece com `~/` ou `/` como externo. Veja [Worktree Directories](../coastfiles/WORKTREE_DIR.md) para detalhes.

Após alterar `worktree_dir`, as instâncias existentes devem ser **recriadas** para que o bind mount entre em vigor:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coast lê o novo Coastfile), mas atribuir a um worktree do Codex requer o bind mount dentro do contêiner.

## O que o Coast faz

- **Bind mount** -- Na criação do contêiner, o Coast monta `~/.codex/worktrees` no contêiner em `/host-external-wt/{index}`.
- **Descoberta** -- `git worktree list --porcelain` tem escopo de repositório, então apenas os worktrees do Codex pertencentes ao projeto atual aparecem, mesmo que o diretório contenha worktrees de muitos projetos.
- **Nomenclatura** -- Worktrees com HEAD destacado aparecem como seu caminho relativo dentro do diretório externo (`a0db/my-app`, `eca7/my-app`). Worktrees baseados em branch mostram o nome da branch.
- **Atribuição** -- `coast assign` remonta `/workspace` a partir do caminho do bind mount externo.
- **Sincronização de arquivos ignorados pelo Git** -- É executada no sistema de arquivos do host com caminhos absolutos, funciona sem o bind mount.
- **Detecção de órfãos** -- O observador do git varre diretórios externos recursivamente, filtrando por ponteiros gitdir em `.git`. Se o Codex excluir um worktree, o Coast remove automaticamente a atribuição da instância.

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

- `.worktrees/` -- Worktrees gerenciados pelo Coast
- `.claude/worktrees/` -- Claude Code (local, sem tratamento especial)
- `~/.codex/worktrees/` -- Codex (externo, com bind mount)

## Limitações

- O Coast descobre e monta worktrees do Codex, mas não os cria nem os exclui.
- O Codex pode limpar worktrees a qualquer momento. A detecção de órfãos do Coast lida com isso de forma elegante.
- Novos worktrees criados por `coast assign` sempre vão para o `default_worktree_dir` local, nunca para um diretório externo.

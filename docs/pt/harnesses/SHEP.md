# Shep

## Configuração rápida

Requer o [Coast CLI](../GETTING_STARTED.md). Copie este prompt para o chat do seu
agente para configurar Coasts automaticamente:

```prompt-copy
shep_setup_prompt.txt
```

Você também pode obter o conteúdo da skill pelo CLI: `coast skills-prompt`.

Após a configuração, **feche e reabra seu editor** para que a nova skill e as
instruções do projeto entrem em vigor.

---

[Shep](https://shep-ai.github.io/cli/) cria worktrees em `~/.shep/repos/{hash}/wt/{branch-slug}`. O hash são os primeiros 16 caracteres hexadecimais do SHA-256 do caminho absoluto do repositório, portanto ele é determinístico por repositório, mas opaco. Todos os worktrees de um determinado repositório compartilham o mesmo hash e são diferenciados pelo subdiretório `wt/{branch-slug}`.

No Shep CLI, `shep feat show <feature-id>` imprime o caminho do worktree, ou
`ls ~/.shep/repos` lista os diretórios de hash por repositório.

Como o hash varia por repositório, Coasts usa um **padrão glob** para descobrir
worktrees do shep sem exigir que o usuário codifique o hash manualmente.

## Configuração

Adicione `~/.shep/repos/*/wt` a `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

O `*` corresponde ao diretório de hash por repositório. Em tempo de execução, o Coasts expande o glob,
encontra o diretório correspondente (por exemplo, `~/.shep/repos/a21f0cda9ab9d456/wt`) e
faz o bind mount dele no contêiner. Veja
[Worktree Directories](../coastfiles/WORKTREE_DIR.md) para detalhes completos sobre padrões
glob.

Após alterar `worktree_dir`, instâncias existentes devem ser **recriadas** para que o bind mount entre em vigor:

```bash
coast rm my-instance
coast build
coast run my-instance
```

A listagem de worktrees é atualizada imediatamente (o Coasts lê o novo Coastfile), mas
atribuir a um worktree do Shep requer o bind mount dentro do contêiner.

## Onde a orientação do Coasts vai

O Shep encapsula o Claude Code internamente, então siga as convenções do Claude Code:

- coloque as regras curtas do Coast Runtime em `CLAUDE.md`
- coloque o fluxo de trabalho reutilizável `/coasts` em `.claude/skills/coasts/SKILL.md` ou
  em `.agents/skills/coasts/SKILL.md` compartilhado
- se este repositório também usa outros harnesses, veja
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) e
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)

## O que o Coasts faz

- **Run** -- `coast run <name>` cria uma nova instância do Coast a partir da build mais recente. Use `coast run <name> -w <worktree>` para criar e atribuir um worktree do Shep em uma única etapa. Veja [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- Na criação do contêiner, o Coasts resolve o glob
  `~/.shep/repos/*/wt` e monta cada diretório correspondente no contêiner em
  `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` é restrito ao escopo do repositório, então apenas
  worktrees pertencentes ao projeto atual aparecem.
- **Naming** -- Worktrees do Shep usam branches nomeados, então aparecem pelo nome
  da branch na UI e no CLI do Coasts (por exemplo, `feat-green-background`).
- **Assign** -- `coast assign` remonta `/workspace` a partir do caminho do bind mount externo.
- **Gitignored sync** -- Executa no sistema de arquivos do host com caminhos absolutos, funciona sem o bind mount.
- **Orphan detection** -- O observador do git varre diretórios externos
  recursivamente, filtrando por ponteiros gitdir de `.git`. Se o Shep excluir um
  worktree, o Coasts desatribui automaticamente a instância.

## Exemplo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
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

- `~/.shep/repos/*/wt` -- Shep (externo, montado por bind via expansão de glob)

## Estrutura de caminho do Shep

```
~/.shep/repos/
  {sha256-of-repo-path-first-16-chars}/
    wt/
      {branch-slug}/     <-- git worktree
      {branch-slug}/
```

Pontos principais:
- Mesmo repositório = mesmo hash todas as vezes (determinístico, não aleatório)
- Repositórios diferentes = hashes diferentes
- Separadores de caminho são normalizados para `/` antes do hash
- O hash pode ser encontrado via `shep feat show <feature-id>` ou `ls ~/.shep/repos`

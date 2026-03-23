# Diretórios de Worktree

O campo `worktree_dir` em `[coast]` controla onde as worktrees do git ficam. O Coast usa worktrees do git para dar a cada instância sua própria cópia da base de código em uma branch diferente, sem duplicar o repositório completo.

## Sintaxe

`worktree_dir` aceita uma única string ou um array de strings:

```toml
# Single directory (default)
worktree_dir = ".worktrees"

# Multiple directories
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

Quando omitido, o padrão é `".worktrees"`.

## Tipos de caminho

### Caminhos relativos

Caminhos que não começam com `~/` ou `/` são resolvidos em relação à raiz do projeto. Estes são os mais comuns e não exigem tratamento especial — eles estão dentro do diretório do projeto e ficam automaticamente disponíveis dentro do contêiner do Coast por meio do bind mount padrão `/host-project`.

```toml
worktree_dir = ".worktrees"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### Caminhos com til (`~`) (externos)

Caminhos que começam com `~/` são expandidos para o diretório home do usuário e tratados como diretórios de worktree **externos**. O Coast adiciona um bind mount separado para que o contêiner possa acessá-los.

```toml
worktree_dir = ["~/.codex/worktrees", ".worktrees"]
```

É assim que você integra com ferramentas que criam worktrees fora da raiz do seu projeto, como o OpenAI Codex (que sempre cria worktrees em `$CODEX_HOME/worktrees`).

### Caminhos absolutos (externos)

Caminhos que começam com `/` também são tratados como externos e recebem seu próprio bind mount.

```toml
worktree_dir = ["/shared/worktrees", ".worktrees"]
```

### Padrões glob (externos)

Caminhos externos podem conter metacaracteres glob (`*`, `?`, `[...]`). O Coast os expande em tempo de execução contra o sistema de arquivos do host, criando um bind mount para cada diretório correspondente.

```toml
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

Isso é útil quando uma ferramenta gera worktrees sob um componente de caminho que varia por projeto (como um hash). O `*` corresponde a qualquer nome de diretório único, então `~/.shep/repos/*/wt` corresponde a `~/.shep/repos/a21f0cda9ab9d456/wt` e a qualquer outro diretório de hash que contenha um subdiretório `wt`.

Sintaxe glob suportada:

- `*` — corresponde a qualquer sequência de caracteres dentro de um único componente de caminho
- `?` — corresponde a qualquer caractere único
- `[abc]` — corresponde a qualquer caractere do conjunto
- `[!abc]` — corresponde a qualquer caractere que não esteja no conjunto

A expansão de glob acontece em todos os lugares onde os diretórios de worktree são resolvidos: criação do contêiner, assign, start, lookup e o monitor do git. As correspondências são ordenadas para garantir uma ordem determinística. Se um glob não corresponder a nenhum diretório, ele será ignorado silenciosamente.

Como outros caminhos externos, o contêiner deve ser recriado (`coast run`) após adicionar um padrão glob para que o bind mount entre em vigor.

## Como os diretórios externos funcionam

Quando o Coast encontra um diretório de worktree externo (caminho com til ou absoluto), três coisas acontecem:

1. **Bind mount do contêiner** — No momento da criação do contêiner (`coast run`), o caminho do host resolvido é montado via bind no contêiner em `/host-external-wt/{index}`, onde `{index}` é a posição no array `worktree_dir`. Isso torna os arquivos externos acessíveis dentro do contêiner.

2. **Filtragem do projeto** — Diretórios externos podem conter worktrees de vários projetos. O Coast usa `git worktree list --porcelain` (que é inerentemente limitado ao repositório atual) para descobrir apenas as worktrees que pertencem a este projeto. O monitor do git também verifica a propriedade lendo o arquivo `.git` de cada worktree e checando se seu ponteiro `gitdir:` resolve de volta para o repositório atual.

3. **Remontagem do workspace** — Quando você usa `coast assign` para uma worktree externa, o Coast remonta `/workspace` a partir do caminho do bind mount externo em vez do caminho usual `/host-project/{dir}/{name}`.

## Nomenclatura de worktrees externas

Worktrees externas com uma branch em checkout aparecem pelo nome da branch, da mesma forma que worktrees locais.

Worktrees externas em **detached HEAD** (comum no Codex) aparecem usando seu caminho relativo dentro do diretório externo. Por exemplo, uma worktree do Codex em `~/.codex/worktrees/a0db/coastguard-platform` aparece como `a0db/coastguard-platform` na UI e na CLI.

## `default_worktree_dir`

Controla qual diretório é usado quando o Coast cria uma **nova** worktree (por exemplo, quando você atribui uma branch que não tem uma worktree existente). O padrão é a primeira entrada em `worktree_dir`.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

Diretórios externos nunca são usados para criar novas worktrees — o Coast sempre cria worktrees em um diretório local (relativo). O campo `default_worktree_dir` só é necessário quando você quer substituir o padrão (primeira entrada).

## Exemplos

### Integração com Codex

O OpenAI Codex cria worktrees em `~/.codex/worktrees/{hash}/{project-name}`. Para torná-las visíveis e atribuíveis no Coast:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Após adicionar isso, as worktrees do Codex aparecem no modal de checkout e na saída de `coast ls`. Você pode atribuir uma instância do Coast a uma worktree do Codex para executar seu código em um ambiente de desenvolvimento completo.

Observação: o contêiner deve ser recriado (`coast run`) após adicionar um diretório externo para que o bind mount entre em vigor. Reiniciar uma instância existente não é suficiente.

### Integração com Claude Code

O Claude Code cria worktrees dentro do projeto em `.claude/worktrees/`. Como este é um caminho relativo (dentro da raiz do projeto), ele funciona como qualquer outro diretório de worktree local — nenhum mount externo é necessário:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

### Integração com Shep

O Shep cria worktrees em `~/.shep/repos/{hash}/wt/{branch-slug}`, onde o hash é por repositório. Use um padrão glob para corresponder ao diretório de hash:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.shep/repos/*/wt"]
```

### Todos os harnesses juntos

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.shep/repos/*/wt"]
```

## Leitura dinâmica do Coastfile

Alterações em `worktree_dir` no seu Coastfile entram em vigor imediatamente para a **listagem** de worktrees (a API e o monitor do git leem o Coastfile em tempo real a partir do disco, não apenas o artefato de build em cache). No entanto, **bind mounts** externos são criados apenas no momento da criação do contêiner, então você precisa recriar a instância para que um diretório externo recém-adicionado possa ser montado.

# Atribuir e Desatribuir

Atribuir e desatribuir controlam para qual worktree uma instância do Coast está apontando. Veja [Filesystem](FILESYSTEM.md) para saber como a troca de worktree funciona no nível de montagem.

## Atribuir

`coast assign` alterna uma instância do Coast para um worktree específico. O Coast cria o worktree se ele ainda não existir, atualiza o código dentro do Coast e reinicia os serviços de acordo com a estratégia de atribuição configurada.

```bash
coast assign dev-1 --worktree feature/oauth
```

```text
Before:
┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘

coast assign dev-1 --worktree feature/oauth

After:
┌─── dev-1 ──────────────────┐
│  branch: feature/oauth     │
│  worktree: feature/oauth   │
│                            │
│  postgres → skipped (none) │
│  web      → hot swapped    │
│  api      → restarted      │
│  worker   → rebuilt        │
└────────────────────────────┘
```

Após atribuir, `dev-1` está executando o branch `feature/oauth` com todos os seus serviços ativos.

## Desatribuir

`coast unassign` alterna uma instância do Coast de volta para a raiz do projeto (seu branch main/master). A associação com o worktree é removida e o Coast volta a executar a partir do repositório principal.

```text
coast unassign dev-1

┌─── dev-1 ──────────────────┐
│  branch: main              │
│  worktree: -               │
└────────────────────────────┘
```

## Estratégias de Atribuição

Quando um Coast é atribuído a um novo worktree, cada serviço precisa saber como lidar com a mudança de código. Você configura isso por serviço no seu [Coastfile](COASTFILE_TYPES.md) em `[assign]`:

```toml
[assign]
default = "restart"

[assign.services]
postgres = "none"
redis = "none"
web = "hot"
worker = "rebuild"
```

```text
coast assign dev-1 --worktree feature/billing

  postgres (strategy: none)    →  skipped, unchanged between branches
  redis (strategy: none)       →  skipped, unchanged between branches
  web (strategy: hot)          →  filesystem swapped, file watcher picks it up
  api (strategy: restart)      →  container restarted
  worker (strategy: rebuild)   →  image rebuilt, container restarted
```

As estratégias disponíveis são:

- **none** — não fazer nada. Use isso para serviços que não mudam entre branches, como Postgres ou Redis.
- **hot** — trocar apenas o filesystem. O serviço continua em execução e capta as mudanças via propagação de montagem e file watchers (ex.: um servidor de desenvolvimento com hot reload).
- **restart** — reiniciar o container do serviço. Use isso para serviços interpretados que só precisam de um reinício do processo. Este é o padrão.
- **rebuild** — reconstruir a imagem do serviço e reiniciar. Use isso quando a mudança de branch afeta o `Dockerfile` ou dependências em tempo de build.

Você também pode especificar gatilhos de rebuild para que um serviço só reconstrua quando arquivos específicos mudarem:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

Se nenhum dos arquivos de gatilho mudou entre branches, o serviço pula o rebuild mesmo que a estratégia esteja definida como `rebuild`.

## Worktrees Excluídos

Se um worktree atribuído for excluído, o daemon `coastd` automaticamente desatribui essa instância de volta para a raiz do repositório Git principal.

---

> **Dica: Reduzindo a latência de atribuição em grandes bases de código**
>
> Por baixo dos panos, a primeira atribuição para um novo worktree inicializa arquivos selecionados ignorados pelo Git nesse worktree, e serviços com `[assign.rebuild_triggers]` podem executar `git diff --name-only` para decidir se um rebuild é necessário. Em grandes bases de código, essa etapa de inicialização e rebuilds desnecessários tendem a dominar o tempo de atribuição.
>
> Use `exclude_paths` no seu Coastfile para reduzir a superfície de inicialização de arquivos ignorados pelo Git, use `"hot"` para serviços com file watchers e mantenha `[assign.rebuild_triggers]` focado em verdadeiras entradas de tempo de build. Se você precisar atualizar manualmente a inicialização de arquivos ignorados para um worktree existente, execute `coast assign --force-sync`. Veja [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) para um guia completo.

---

> **Atribuição remota**
>
> Para [coasts remotos](REMOTES.md), `coast assign` faz rsync do conteúdo do novo worktree para o host remoto antes de encaminhar a solicitação de atribuição para `coast-service`. As mesmas estratégias de atribuição se aplicam. Após o rsync inicial, o mutagen continua sincronizando alterações de arquivos em tempo real.

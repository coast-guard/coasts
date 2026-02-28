# Atribuir e Desatribuir

Atribuir e desatribuir controlam para qual worktree uma instância do Coast está apontando. Veja [Filesystem](FILESYSTEM.md) para saber como a troca de worktree funciona no nível de montagem.

## Atribuir

`coast assign` muda uma instância do Coast para um worktree específico. O Coast cria o worktree caso ele ainda não exista, atualiza o código dentro do Coast e reinicia os serviços de acordo com a estratégia de atribuição configurada.

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

`coast unassign` muda uma instância do Coast de volta para a raiz do projeto (seu branch main/master). A associação com o worktree é removida e o Coast volta a executar a partir do repositório primário.

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

- **none** — não fazer nada. Use isto para serviços que não mudam entre branches, como Postgres ou Redis.
- **hot** — trocar apenas o sistema de arquivos. O serviço continua em execução e capta as mudanças via propagação de montagem e observadores de arquivo (por exemplo, um servidor de desenvolvimento com hot reload).
- **restart** — reiniciar o contêiner do serviço. Use isto para serviços interpretados que só precisam de um reinício do processo. Este é o padrão.
- **rebuild** — reconstruir a imagem do serviço e reiniciar. Use isto quando a mudança de branch afeta o `Dockerfile` ou dependências de tempo de build.

Você também pode especificar gatilhos de rebuild para que um serviço só reconstrua quando arquivos específicos mudarem:

```toml
[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
```

Se nenhum dos arquivos de gatilho tiver mudado entre branches, o serviço pula a reconstrução mesmo que a estratégia esteja definida como `rebuild`.

## Worktrees Excluídos

Se um worktree atribuído for deletado, o daemon `coastd` automaticamente desatribui aquela instância de volta para a raiz do repositório Git principal.

---

> **Dica: Reduzindo a latência de atribuição em bases de código grandes**
>
> Por baixo dos panos, o Coast executa `git ls-files` sempre que um worktree é montado ou desmontado. Em bases de código grandes ou repositórios com muitos arquivos, isso pode adicionar uma latência perceptível às operações de atribuir e desatribuir.
>
> Se partes da sua base de código não precisam ser reconstruídas entre atribuições, você pode dizer ao Coast para ignorá-las usando `exclude_paths` no seu Coastfile:
>
> ```toml
> [assign]
> default = "restart"
> exclude_paths = ["docs", "scripts", "test-fixtures"]
> ```
>
> Caminhos listados em `exclude_paths` são ignorados durante o diff de arquivos, o que pode acelerar significativamente os tempos de atribuição.

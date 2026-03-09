# Otimizações de Desempenho

O Coast foi projetado para tornar a troca de branches rápida, mas em monorepos grandes o comportamento padrão ainda pode introduzir latência. Esta página cobre as alavancas disponíveis no seu Coastfile e, mais importante, quais partes de `coast assign` elas realmente afetam.

## Por que o Assign Pode Ser Lento

`coast assign` faz várias coisas ao alternar um Coast para um novo worktree:

```text
coast assign dev-1 --worktree feature/payments

  1. classify services and optional rebuild-trigger diff
  2. stop affected services
  3. create git worktree (if new)
  4. bootstrap gitignored files into the worktree (first assign only)
  5. remount /workspace
  6. recreate/restart containers
  7. rebuild images for services using "rebuild"
  8. wait for healthy
```

Os maiores custos variáveis geralmente são o **bootstrap inicial de arquivos ignorados pelo git**, **reinicializações de containers** e **rebuilds de imagens**. O diff opcional de branch usado para gatilhos de rebuild é muito mais barato, mas ainda pode se acumular se você apontá-lo para conjuntos amplos de gatilhos.

### Bootstrap de Arquivos Ignorados pelo Git

Quando um worktree é criado pela primeira vez, o Coast faz o bootstrap de arquivos selecionados ignorados pelo git a partir da raiz do projeto para esse worktree.

A sequência é:

1. Executar `git ls-files --others --ignored --exclude-standard` no host para enumerar arquivos ignorados.
2. Filtrar diretórios comuns e pesados, além de quaisquer `exclude_paths` configurados.
3. Executar `rsync --files-from` com `--link-dest` para que os arquivos selecionados sejam hardlinkados no worktree em vez de copiados byte a byte.
4. Registrar o bootstrap bem-sucedido em metadados internos do worktree para que assigns posteriores para o mesmo worktree possam pulá-lo.

Se o `rsync` não estiver disponível, o Coast recorre a um pipeline de `tar`.

Diretórios grandes como `node_modules`, `.git`, `dist`, `target`, `.next`, `.nuxt`, `.cache`, `.worktrees` e `.coasts` são excluídos automaticamente. Diretórios grandes de dependências devem ser tratados por caches ou volumes de serviço, e não por esta etapa genérica de bootstrap.

Como a lista de arquivos é gerada antecipadamente, o `rsync` trabalha a partir de uma lista direcionada em vez de vasculhar cegamente todo o repositório. Mesmo assim, repos com conjuntos muito grandes de arquivos ignorados ainda podem pagar um custo perceptível de bootstrap único quando um worktree é criado pela primeira vez. Se você precisar atualizar esse bootstrap manualmente, execute `coast assign --force-sync`.

### Diff de Gatilho de Rebuild

O Coast só calcula um diff de branch quando `[assign.rebuild_triggers]` está configurado. Nesse caso, ele executa:

```bash
git diff --name-only <previous>..<worktree>
```

O resultado é usado para rebaixar um serviço de `rebuild` para `restart` quando nenhum de seus arquivos de gatilho mudou.

Isso é muito mais restrito do que o antigo modelo de "diff de todos os arquivos rastreados em todo assign". Se você não configurar rebuild triggers, não há nenhuma etapa de diff de branch aqui.

No momento, `exclude_paths` não altera esse diff. Mantenha suas listas de gatilhos focadas em verdadeiras entradas de build, como Dockerfiles, lockfiles e manifests de pacotes.

## `exclude_paths` — A Principal Alavanca para Novos Worktrees

A opção `exclude_paths` no seu Coastfile diz ao Coast para pular árvores de diretórios inteiras ao montar a lista de arquivos ignorados pelo git para o bootstrap de um novo worktree.

```toml
[assign]
default = "none"
exclude_paths = [
    "docs",
    "scripts",
    "test-fixtures",
    "apps/mobile",
]
```

Arquivos sob paths excluídos ainda estão presentes no worktree se o Git os rastrear. O Coast apenas evita gastar tempo enumerando e hardlinkando arquivos ignorados sob essas árvores durante o bootstrap inicial.

Isso é mais impactante quando a raiz do seu repo contém grandes diretórios ignorados com os quais seus serviços em execução não se importam: apps não relacionados, caches vendorizados, fixtures de teste, docs geradas e outras árvores pesadas.

Se você está repetidamente fazendo assign para o mesmo worktree já sincronizado, `exclude_paths` importa menos porque o bootstrap é pulado. Nesse caso, as escolhas de restart/rebuild de serviços se tornam o fator dominante.

### Escolhendo o que Excluir

Comece fazendo um perfil dos seus arquivos ignorados:

```bash
git ls-files --others --ignored --exclude-standard | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

Se você também quiser uma visão do layout rastreado para ajustar rebuild-trigger, use:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

**Mantenha** diretórios que:
- Contêm código-fonte montado em serviços em execução
- Contêm bibliotecas compartilhadas importadas por esses serviços
- Contêm arquivos gerados ou caches que seu runtime realmente precisa no primeiro boot
- São referenciados em `[assign.rebuild_triggers]`

**Exclua** diretórios que:
- Pertencem a apps ou serviços que não estão rodando no seu Coast
- Contêm documentação, scripts, configs de CI ou ferramentas não relacionadas ao runtime
- Guardam caches grandes ignorados que já são preservados em outro lugar, como caches dedicados de serviço ou volumes compartilhados

### Exemplo: Monorepo com Vários Apps

Um monorepo com muitos diretórios no topo, mas apenas um subconjunto importa para os serviços em execução neste Coast:

```text
  13,000  bookface/         ← active
   7,000  ycinternal/       ← active
     850  shared/           ← used by both
   3,800  .yarn/            ← excludable
   2,500  startupschool/    ← excludable
     500  misc/             ← excludable
     300  ycapp/            ← excludable
     ...  (12 more dirs)    ← excludable
```

```toml
[assign]
default = "none"
exclude_paths = [
    ".yarn",
    "startupschool",
    "misc",
    "ycapp",
    "apply",
    "cli",
    "deploy",
    "lambdas",
    # ... any other directories not needed by active services
]
```

Isso mantém o bootstrap inicial do worktree focado nos diretórios de que os serviços em execução realmente precisam, em vez de gastar tempo com árvores ignoradas não relacionadas.

## Remova Serviços Inativos de `[assign.services]`

Se o seu `COMPOSE_PROFILES` só inicia um subconjunto de serviços, remova serviços inativos de `[assign.services]`. O Coast avalia a estratégia de assign para cada serviço listado, e reiniciar ou rebuildar um serviço que não está rodando é trabalho desperdiçado.

```toml
# Bad — restarts services that aren't running
[assign.services]
web = "restart"
api = "restart"
mobile-api = "restart"   # not in COMPOSE_PROFILES
batch-worker = "restart"  # not in COMPOSE_PROFILES

# Good — only services that are actually running
[assign.services]
web = "restart"
api = "restart"
```

O mesmo se aplica a `[assign.rebuild_triggers]` — remova entradas para serviços que não estão ativos.

## Use `"hot"` Quando Possível

A estratégia `"hot"` pula completamente o restart do container. O [remount do filesystem](FILESYSTEM.md) troca o código sob `/workspace` e o file watcher do serviço (Vite, webpack, nodemon, air, etc.) detecta as mudanças automaticamente.

```toml
[assign.services]
web = "hot"        # Vite/webpack dev server with HMR
api = "restart"    # Rails/Go — needs a process restart
```

`"hot"` é mais rápido do que `"restart"` porque evita o ciclo de parar/iniciar do container. Use-o para qualquer serviço que execute um dev server com file watching. Reserve `"restart"` para serviços que carregam o código na inicialização e não monitoram mudanças (a maioria dos apps Rails, Go e Java).

## Use `"rebuild"` com Gatilhos

Se a estratégia padrão de um serviço é `"rebuild"`, toda troca de branch faz rebuild da imagem Docker — mesmo que nada que afete a imagem tenha mudado. Adicione `[assign.rebuild_triggers]` para condicionar o rebuild a arquivos específicos:

```toml
[assign.services]
worker = "rebuild"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json", "package-lock.json"]
```

Se nenhum dos arquivos de gatilho mudou entre branches, o Coast pula o rebuild e recua para um restart. Isso evita builds caros de imagem em mudanças rotineiras de código.

## Resumo

| Otimização | Impacto | Afeta | Quando usar |
|---|---|---|---|
| `exclude_paths` | Alto | bootstrap inicial de arquivos ignorados pelo git | Repos com grandes árvores ignoradas de que seu Coast não precisa |
| Remover serviços inativos | Médio | restart/recreate de serviços | Quando `COMPOSE_PROFILES` limita quais serviços rodam |
| Estratégia `"hot"` | Alto | restart de container | Serviços com file watchers (Vite, webpack, nodemon, air) |
| `rebuild_triggers` | Alto | rebuilds de imagem + diff de branch opcional | Serviços usando `"rebuild"` que só precisam disso para mudanças de infra |

Se novos worktrees são lentos para fazer assign pela primeira vez, comece com `exclude_paths`. Se assigns repetidos são lentos, foque em `hot` vs `restart`, remova serviços inativos e mantenha `rebuild_triggers` enxuto.

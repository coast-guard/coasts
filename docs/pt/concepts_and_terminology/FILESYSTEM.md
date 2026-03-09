# Sistema de arquivos

Sua máquina host e todas as instâncias do Coast compartilham os mesmos arquivos do projeto. A raiz do projeto no host é montada com leitura e escrita no contêiner DinD em `/host-project`, e o Coast faz um bind-mount da árvore de trabalho ativa em `/workspace`. É isso que torna possível que um agente rodando na sua máquina host edite código enquanto os serviços dentro do Coast capturam as alterações em tempo real.

## A Montagem Compartilhada

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

A raiz do projeto no host é montada com leitura e escrita em `/host-project` dentro do [contêiner DinD](RUNTIMES_AND_SERVICES.md) quando o contêiner é criado. Depois que o contêiner inicia, um `mount --bind /host-project /workspace` dentro do contêiner cria o caminho de trabalho `/workspace` com propagação de montagem compartilhada (`mount --make-rshared`), para que serviços do compose interno que fazem bind-mount de subdiretórios de `/workspace` vejam o conteúdo correto.

Essa abordagem em duas etapas existe por um motivo: o bind mount do Docker em `/host-project` é fixo na criação do contêiner e não pode ser alterado sem recriar o contêiner. Mas o bind mount do Linux em `/workspace` dentro do contêiner pode ser desmontado e refeito apontando para um subdiretório diferente — uma worktree — sem tocar no ciclo de vida do contêiner. É isso que torna `coast assign` rápido.

`/workspace` é leitura e escrita. As alterações de arquivos fluem instantaneamente nos dois sentidos. Salve um arquivo no host e um servidor de desenvolvimento dentro do Coast o captura. Crie um arquivo dentro do Coast e ele aparece no host.

## Agentes no Host e Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

Como o sistema de arquivos é compartilhado, um agente de codificação por IA rodando no host pode editar arquivos livremente e os serviços em execução dentro do Coast veem as alterações imediatamente. O agente não precisa rodar dentro do contêiner do Coast — ele opera a partir do host normalmente.

Quando o agente precisa de informações de runtime — logs, status de serviços, saída de testes — ele chama comandos da CLI do Coast a partir do host:

- `coast logs dev-1 --service web --tail 50` para a saída do serviço (veja [Logs](LOGS.md))
- `coast ps dev-1` para o status do serviço (veja [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` para executar comandos dentro do Coast (veja [Exec & Docker](EXEC_AND_DOCKER.md))

Esta é a vantagem arquitetural fundamental: **a edição de código acontece no host, o runtime acontece no Coast, e o sistema de arquivos compartilhado faz a ponte entre eles.** O agente no host nunca precisa estar "dentro" do Coast para fazer seu trabalho.

## Troca de Worktree

Quando `coast assign` alterna um Coast para uma worktree diferente, ele remonta `/workspace` para apontar para aquela worktree do git em vez da raiz do projeto:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

A worktree é criada no host em `{project_root}/.worktrees/{worktree_name}`. O nome do diretório `.worktrees` é configurável via `worktree_dir` no seu Coastfile e deve estar no seu `.gitignore`.

Se a worktree for nova, o Coast faz o bootstrap de arquivos selecionados ignorados pelo git a partir da raiz do projeto antes da remontagem. Ele enumera arquivos ignorados com `git ls-files --others --ignored --exclude-standard`, filtra diretórios comuns e pesados mais quaisquer `exclude_paths` configurados, e então usa `rsync --files-from` com `--link-dest` para criar hardlinks dos arquivos selecionados dentro da worktree. O Coast registra esse bootstrap em metadados internos da worktree e o pula em atribuições posteriores para a mesma worktree, a menos que você explicitamente o atualize com `coast assign --force-sync`.

Dentro do contêiner, `/workspace` é lazy-unmounted e refeito apontando para o subdiretório da worktree em `/host-project/.worktrees/{branch_name}`. Essa remontagem é rápida — ela não recria o contêiner DinD nem reinicia o daemon Docker interno. Serviços do compose e serviços bare ainda podem ser recriados ou reiniciados após a remontagem para que seus bind mounts resolvam através do novo `/workspace`.

Grandes diretórios de dependências como `node_modules` não fazem parte deste caminho genérico de bootstrap. Eles normalmente são tratados por meio de caches ou volumes específicos do serviço.

Se você usar `[assign.rebuild_triggers]`, o Coast também executa `git diff --name-only <previous>..<worktree>` no host para decidir se um serviço marcado como `rebuild` pode ser rebaixado para `restart`. Veja [Assign and Unassign](ASSIGN.md) e [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) para os detalhes que afetam a latência do assign.

`coast unassign` reverte `/workspace` de volta para `/host-project` (a raiz do projeto). `coast start` após um stop reaplica a montagem correta com base em se a instância tem uma worktree atribuída.

## Todas as Montagens

Todo contêiner do Coast tem estas montagens:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Raiz do projeto ou worktree. Alternável no assign. |
| `/host-project` | Docker bind mount | RW | Raiz bruta do projeto. Fixa na criação do contêiner. |
| `/image-cache` | Docker bind mount | RO | Tarballs OCI pré-baixados de `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Artefato de build com arquivos compose reescritos. |
| `/coast-override` | Docker bind mount | RO | Overrides de compose gerados para [serviços compartilhados](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Estado do daemon Docker interno. Persiste entre remoções do contêiner. |

As montagens somente leitura são infraestrutura — elas carregam o artefato de build, imagens em cache e overrides de compose que o Coast gera. Você interage com elas indiretamente por meio de `coast build` e do Coastfile. As montagens de leitura e escrita são onde seu código vive e onde o daemon interno armazena seu estado.

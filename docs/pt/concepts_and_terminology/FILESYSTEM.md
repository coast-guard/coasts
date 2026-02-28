# Sistema de Arquivos

A sua máquina host e cada instância do Coast compartilham os mesmos arquivos do projeto. A raiz do projeto no host é montada via bind no container DinD em `/workspace`, então edições no host aparecem instantaneamente dentro do Coast e vice-versa. É isso que torna possível que um agente rodando na sua máquina host edite código enquanto serviços dentro do Coast capturam as mudanças em tempo real.

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

A raiz do projeto no host é montada com leitura e escrita em `/host-project` dentro do [container DinD](RUNTIMES_AND_SERVICES.md) quando o container é criado. Depois que o container inicia, um `mount --bind /host-project /workspace` dentro do container cria o caminho de trabalho `/workspace` com propagação de montagem compartilhada (`mount --make-rshared`), para que serviços compose internos que fazem bind-mount de subdiretórios de `/workspace` vejam o conteúdo correto.

Essa abordagem em duas etapas existe por um motivo: o bind mount do Docker em `/host-project` é fixo na criação do container e não pode ser alterado sem recriar o container. Mas o bind mount do Linux em `/workspace` dentro do container pode ser desmontado e re-montado para um subdiretório diferente — um worktree — sem mexer no ciclo de vida do container. É isso que torna `coast assign` rápido.

`/workspace` é leitura e escrita. Mudanças de arquivos fluem instantaneamente nos dois sentidos. Salve um arquivo no host e um servidor de desenvolvimento dentro do Coast o captura. Crie um arquivo dentro do Coast e ele aparece no host.

## Agentes no Host e o Coast

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

Como o sistema de arquivos é compartilhado, um agente de codificação por IA rodando no host pode editar arquivos livremente e os serviços em execução dentro do Coast veem as mudanças imediatamente. O agente não precisa rodar dentro do container do Coast — ele opera a partir do host normalmente.

Quando o agente precisa de informações de runtime — logs, status de serviço, saída de testes — ele chama comandos da CLI do Coast a partir do host:

- `coast logs dev-1 --service web --tail 50` para saída do serviço (veja [Logs](LOGS.md))
- `coast ps dev-1` para status do serviço (veja [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` para executar comandos dentro do Coast (veja [Exec & Docker](EXEC_AND_DOCKER.md))

Esta é a vantagem arquitetural fundamental: **a edição de código acontece no host, o runtime acontece no Coast, e o sistema de arquivos compartilhado faz a ponte entre eles.** O agente no host nunca precisa estar "dentro" do Coast para fazer seu trabalho.

## Troca de Worktree

Quando `coast assign` troca um Coast para um worktree diferente, ele remonta `/workspace` para apontar para aquele worktree do git em vez da raiz do projeto:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

O worktree é criado no host em `{project_root}/.worktrees/{worktree_name}`. O nome do diretório `.worktrees` é configurável via `worktree_dir` no seu Coastfile e deve estar no seu `.gitignore`.

Dentro do container, `/workspace` é desmontado (lazy-unmount) e re-montado para o subdiretório do worktree em `/host-project/.worktrees/{branch_name}`. Essa remontagem é rápida — ela não recria o container DinD nem reinicia o daemon Docker interno. Serviços compose internos são recriados para que seus bind mounts sejam resolvidos através do novo `/workspace`.

Arquivos ignorados pelo git como `node_modules` são sincronizados da raiz do projeto para o worktree via rsync com hardlinks, então a configuração inicial é quase instantânea mesmo para grandes árvores de dependências.

No macOS, E/S de arquivos entre o host e a VM do Docker tem overhead inerente. O Coast executa `git ls-files` durante assign e unassign para diff do worktree, e em grandes bases de código isso pode adicionar latência perceptível. Se partes do seu projeto não precisam ser diffadas entre assigns (docs, fixtures de teste, scripts), você pode excluí-las com `exclude_paths` no seu Coastfile para reduzir esse overhead. Veja [Assign and Unassign](ASSIGN.md) para detalhes.

`coast unassign` reverte `/workspace` de volta para `/host-project` (a raiz do projeto). `coast start` após um stop reaplica a montagem correta com base em se a instância tem ou não um worktree atribuído.

## Todas as Montagens

Todo container do Coast tem estas montagens:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Raiz do projeto ou worktree. Comutável no assign. |
| `/host-project` | Docker bind mount | RW | Raiz bruta do projeto. Fixa na criação do container. |
| `/image-cache` | Docker bind mount | RO | Tarballs OCI pré-baixadas de `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Artefato de build com arquivos compose reescritos. |
| `/coast-override` | Docker bind mount | RO | Overrides do compose gerados para [shared services](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Estado do daemon Docker interno. Persiste através da remoção do container. |

As montagens somente leitura são infraestrutura — elas carregam o artefato de build, imagens em cache e overrides do compose que o Coast gera. Você interage com elas indiretamente por meio de `coast build` e do Coastfile. As montagens de leitura e escrita são onde seu código vive e onde o daemon interno armazena seu estado.

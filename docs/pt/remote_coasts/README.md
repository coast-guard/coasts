# Coasts Remotas

> **Beta.** As coasts remotas são totalmente funcionais, mas as flags da CLI, o esquema do Coastfile e a API do coast-service podem mudar em versões futuras. Se você descobrir um bug ou defeito, por favor abra um pull request ou registre uma issue.

As coasts remotas executam seus serviços em uma máquina remota, mantendo a experiência de desenvolvimento idêntica às coasts locais. `coast run`, `coast assign`, `coast exec`, `coast ps`, `coast logs` e todos os outros comandos funcionam da mesma forma. O daemon detecta que a instância é remota e encaminha transparentemente as operações por meio de um túnel SSH.

## Por que Remota

As coasts locais executam tudo no seu laptop. Cada instância de coast executa um contêiner Docker-in-Docker completo com toda a sua stack compose: servidor web, API, workers, bancos de dados, caches, servidor de e-mail. Isso funciona até que seu laptop fique sem RAM ou espaço em disco.

Um projeto full-stack com vários serviços pode consumir uma quantidade significativa de RAM por coast. Execute algumas coasts em paralelo e você atingirá o limite do seu laptop.

```text
  coast-1         coast-2         coast-3         coast-4
  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ worker   │   │ worker   │   │ worker   │   │ worker   │
  │ api      │   │ api      │   │ api      │   │ api      │
  │ admin    │   │ admin    │   │ admin    │   │ admin    │
  │ web      │   │ web      │   │ web      │   │ web      │
  │ mailhog  │   │ mailhog  │   │ mailhog  │   │ mailhog  │
  │          │   │          │   │          │   │          │
  │ 12 GB    │   │ 12 GB    │   │ 12 GB    │   │ 12 GB    │
  └──────────┘   └──────────┘   └──────────┘   └──────────┘

  Total: 48 GB RAM no seu laptop
```

As coasts remotas permitem escalar horizontalmente movendo algumas das suas coasts para máquinas remotas. Os contêineres DinD, os serviços compose e as builds de imagem são executados remotamente, enquanto seu editor e agentes permanecem locais. Serviços compartilhados como Postgres e Redis também permanecem locais, mantendo seu banco de dados sincronizado entre instâncias locais e remotas por meio de túneis reversos SSH.

```text
  Sua Máquina                         Servidor Remoto
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agentes   │             │  coast-1 (todos os serviços) │
  │                     │  SSH        │  coast-2 (todos os serviços) │
  │  serviços compartilhados │──túneis──▶ │  coast-3 (todos os serviços) │
  │  (postgres, redis)  │             │  coast-4 (todos os serviços) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: leve                       Servidor: 64 GB RAM, 16 CPU
```

Escale horizontalmente seu runtime de localhost.

## Início Rápido

```bash
# 1. Register a remote machine
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote test my-vm

# 2. Build on the remote (uses remote's native architecture)
coast build --type remote

# 3. Run a remote coast
coast run dev-1 --type remote

# 4. Everything works as usual
coast ps dev-1
coast exec dev-1 -- bash
coast assign dev-1 --worktree feature/x
coast checkout dev-1
```

Para instruções completas de configuração, incluindo preparação do host e implantação do coast-service, consulte [Setup](SETUP.md).

## Referência

| Page | What it covers |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | A divisão em dois contêineres (shell coast + remote coast), camada de túnel SSH, cadeia de encaminhamento de portas e como o daemon roteia requisições |
| [Setup](SETUP.md) | Requisitos do host, implantação do coast-service, registro de remotos e início rápido de ponta a ponta |
| [File Sync](FILE_SYNC.md) | rsync para transferência em massa, mutagen para sincronização contínua, ciclo de vida em run/assign/stop, exclusões e tratamento de condições de corrida |
| [Builds](BUILDS.md) | Build no remoto para arquitetura nativa, transferência de artefatos, o symlink `latest-remote`, reutilização de arquitetura e auto-pruning |
| [CLI and Configuration](CLI.md) | Comandos `coast remote`, configuração de `Coastfile.remote`, gerenciamento de disco e `coast remote prune` |

## Veja Também

- [Remotes](../concepts_and_terminology/REMOTES.md) -- visão geral do conceito no glossário de terminologia
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- como serviços compartilhados locais são conectados por túnel reverso para coasts remotas
- [Ports](../concepts_and_terminology/PORTS.md) -- como a camada de túnel SSH se encaixa no modelo de portas canônicas/dinâmicas

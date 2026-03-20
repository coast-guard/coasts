# Coasts

Uma Coast é um ambiente de execução autocontido do seu projeto. Ela é executada dentro de um [container Docker-in-Docker](RUNTIMES_AND_SERVICES.md), e vários serviços (seu servidor web, banco de dados, cache, etc.) podem ser executados dentro de uma única instância de Coast.

```text
┌─── Coast: dev-1 (branch: feature/oauth) ──────────────┐
│                                                       │
│   ┌─────────┐   ┌──────────┐   ┌─────────┐            │
│   │   web   │   │ postgres │   │  redis  │            │
│   │  :3000  │   │  :5432   │   │  :6379  │            │
│   └─────────┘   └──────────┘   └─────────┘            │
│                                                       │
│   dynamic ports: 62217, 55681, 56905                  │
└───────────────────────────────────────────────────────┘

┌─── Coast: dev-2 (branch: feature/billing) ────────────┐
│                                                       │
│   ┌─────────┐   ┌──────────┐   ┌─────────┐            │
│   │   web   │   │ postgres │   │  redis  │            │
│   │  :3000  │   │  :5432   │   │  :6379  │            │
│   └─────────┘   └──────────┘   └─────────┘            │
│                                                       │
│   dynamic ports: 63104, 57220, 58412                  │
└───────────────────────────────────────────────────────┘
```

Cada Coast expõe seu próprio conjunto de [portas dinâmicas](PORTS.md) para a máquina host, o que significa que você pode acessar qualquer Coast em execução a qualquer momento, independentemente do que mais estiver em execução.

Quando você faz [checkout](CHECKOUT.md) de uma Coast, as portas canônicas do projeto são mapeadas para ela — então `localhost:3000` acessa a Coast em checkout em vez de uma porta dinâmica.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1 web
localhost:5432  ──→  dev-1 postgres
localhost:6379  ──→  dev-1 redis

coast checkout dev-2   (instant swap)

localhost:3000  ──→  dev-2 web
localhost:5432  ──→  dev-2 postgres
localhost:6379  ──→  dev-2 redis
```

Normalmente, uma Coast é [atribuída a uma worktree específica](ASSIGN.md). É assim que você executa várias worktrees do mesmo projeto em paralelo sem conflitos de porta ou colisões de volume.

Você cria instâncias de Coast com [`coast run`](RUN.md). Cabe a você iniciar e encerrar Coasts como achar melhor. Você provavelmente não gostaria de ter 20 Coasts de um projeto intensivo em memória em execução ao mesmo tempo, mas cada um sabe de si.

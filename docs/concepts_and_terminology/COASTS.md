# Coasts

A Coast is a self-contained runtime of your project. It runs inside a [Docker-in-Docker container](RUNTIMES_AND_SERVICES.md), and multiple services (your web server, database, cache, etc.) can all run inside a single Coast instance.

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

Each Coast exposes its own set of [dynamic ports](PORTS.md) to the host machine, meaning you can access any running Coast at any time regardless of what else is running.

When you [check out](CHECKOUT.md) a Coast, the project's canonical ports are mapped to it — so `localhost:3000` hits the checked-out Coast rather than a dynamic port.

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

Typically a Coast is [assigned to a specific worktree](ASSIGN.md). This is how you run multiple worktrees of the same project in parallel without port conflicts or volume collisions.

You create Coast instances with [`coast run`](RUN.md). It is up to you to spin Coasts up and down as you see fit. You probably would not want 20 Coasts of a memory-intensive project running at once, but to each their own.

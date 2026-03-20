# Costas

Una Costa es un entorno de ejecución autocontenido de tu proyecto. Se ejecuta dentro de un [contenedor Docker-in-Docker](RUNTIMES_AND_SERVICES.md), y varios servicios (tu servidor web, base de datos, caché, etc.) pueden ejecutarse dentro de una sola instancia de Costa.

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

Cada Costa expone su propio conjunto de [puertos dinámicos](PORTS.md) a la máquina host, lo que significa que puedes acceder a cualquier Costa en ejecución en cualquier momento sin importar qué más esté ejecutándose.

Cuando [cambias](CHECKOUT.md) a una Costa, los puertos canónicos del proyecto se asignan a ella, por lo que `localhost:3000` apunta a la Costa seleccionada en lugar de a un puerto dinámico.

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

Normalmente, una Costa se [asigna a un worktree específico](ASSIGN.md). Así es como ejecutas múltiples worktrees del mismo proyecto en paralelo sin conflictos de puertos ni colisiones de volúmenes.

Creas instancias de Costa con [`coast run`](RUN.md). Depende de ti levantar y bajar Costas como mejor te parezca. Probablemente no querrías tener 20 Costas de un proyecto intensivo en memoria ejecutándose a la vez, pero cada quien con lo suyo.

# Coasts

Coast는 프로젝트의 자체 포함된 런타임입니다. [Docker-in-Docker 컨테이너](RUNTIMES_AND_SERVICES.md) 안에서 실행되며, 여러 서비스(웹 서버, 데이터베이스, 캐시 등)가 모두 하나의 Coast 인스턴스 안에서 실행될 수 있습니다.

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

각 Coast는 호스트 머신에 자체적인 [동적 포트](PORTS.md) 세트를 노출하므로, 다른 무엇이 실행 중이든 상관없이 언제든지 실행 중인 어떤 Coast에도 접근할 수 있습니다.

Coast를 [체크아웃](CHECKOUT.md)하면 프로젝트의 표준 포트가 해당 Coast에 매핑됩니다 — 따라서 `localhost:3000`은 동적 포트가 아니라 체크아웃된 Coast를 가리킵니다.

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

일반적으로 Coast는 [특정 worktree에 할당](ASSIGN.md)됩니다. 이것이 포트 충돌이나 볼륨 충돌 없이 동일한 프로젝트의 여러 worktree를 병렬로 실행하는 방법입니다.

Coast를 올리고 내리는 것은 적절하다고 판단하는 대로 여러분에게 달려 있습니다. 메모리를 많이 쓰는 프로젝트에서 Coast 20개를 한 번에 실행하고 싶지는 않겠지만, 사람마다 다를 수 있습니다.

# 공유 서비스

공유 서비스는 Coast 내부가 아니라 호스트 Docker 데몬에서 실행되는 데이터베이스 및 인프라 컨테이너(Postgres, Redis, MongoDB 등)입니다. Coast 인스턴스는 브리지 네트워크를 통해 이들에 연결되므로, 모든 Coast는 동일한 호스트 볼륨에 있는 동일한 서비스와 통신합니다.

![Shared services in Coastguard](../../assets/coastguard-shared-services.png)
*호스트에서 관리되는 Postgres, Redis, MongoDB를 보여주는 Coastguard 공유 서비스 탭.*

## 작동 방식

Coastfile에서 공유 서비스를 선언하면, Coast는 이를 호스트 데몬에서 시작하고 각 Coast 컨테이너 내부에서 실행되는 compose 스택에서는 제거합니다. 그런 다음 Coast는 서비스 이름 기반 트래픽을 공유 컨테이너로 다시 라우팅하도록 구성되며, 이때 Coast 내부에서는 해당 서비스의 컨테이너 측 포트가 그대로 유지됩니다.

```text
Host Docker daemon
  |
  +--> postgres (host volume: infra_postgres_data)
  +--> redis    (host volume: infra_redis_data)
  +--> mongodb  (host volume: infra_mongodb_data)
  |
  +--> Coast: dev-1  --bridge network--> host postgres, redis, mongodb
  +--> Coast: dev-2  --bridge network--> host postgres, redis, mongodb
```

공유 서비스는 기존 호스트 볼륨을 재사용하므로, 로컬에서 `docker-compose up`을 실행하며 이미 가지고 있던 데이터는 즉시 Coasts에서 사용할 수 있습니다.

이 구분은 매핑된 포트를 사용할 때 중요합니다:

```toml
[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432"]
```

- 호스트에서는 공유 서비스가 `localhost:5433`으로 게시됩니다.
- 각 Coast 내부에서는 앱 컨테이너가 계속 `postgis:5432`에 연결합니다.
- `5432` 같은 단일 정수는 동일 매핑 `"5432:5432"`의 축약형입니다.

## 공유 서비스를 사용해야 하는 경우

- 프로젝트에 로컬 데이터베이스에 연결하는 MCP 통합이 있는 경우 — 공유 서비스를 사용하면 동적 포트 검색 없이도 계속 작동할 수 있습니다. 공유 서비스를 도구가 이미 사용 중인 동일한 호스트 포트로 게시하면(예: `ports = [5432]`), 해당 도구는 변경 없이 계속 작동합니다. 다른 호스트 포트로 게시하면(예: `"5433:5432"`), 호스트 측 도구는 그 호스트 포트를 사용해야 하며 Coasts는 계속 컨테이너 포트를 사용합니다.
- 자체 데이터베이스 컨테이너를 실행할 필요가 없으므로 더 가벼운 Coast 인스턴스를 원할 때.
- Coast 인스턴스 간 데이터 격리가 필요하지 않을 때(모든 인스턴스가 동일한 데이터를 봄).
- 호스트에서 코딩 에이전트를 실행 중이고([Filesystem](FILESYSTEM.md) 참고), 데이터베이스 상태에 [`coast exec`](EXEC_AND_DOCKER.md)를 거치지 않고 접근하게 하려는 경우. 공유 서비스를 사용하면 에이전트의 기존 데이터베이스 도구와 MCP가 변경 없이 작동합니다.

격리가 필요한 경우의 대안은 [Volume Topology](VOLUMES.md) 페이지를 참고하세요.

## 볼륨 식별 경고

Docker 볼륨 이름은 항상 전역적으로 고유하지 않습니다. 여러 다른 프로젝트에서 `docker-compose up`을 실행하면, Coast가 공유 서비스에 연결하는 호스트 볼륨이 예상한 것과 다를 수 있습니다.

공유 서비스와 함께 Coast를 시작하기 전에, 마지막으로 실행한 `docker-compose up`이 Coasts와 함께 사용하려는 프로젝트에서 실행된 것이 맞는지 확인하세요. 이렇게 하면 호스트 볼륨이 Coastfile이 기대하는 것과 일치하게 됩니다.

## 문제 해결

공유 서비스가 잘못된 호스트 볼륨을 가리키는 것처럼 보이는 경우:

1. [Coastguard](COASTGUARD.md) UI(`coast ui`)를 엽니다.
2. **Shared Services** 탭으로 이동합니다.
3. 영향을 받는 서비스를 선택하고 **Remove**를 클릭합니다.
4. **Refresh Shared Services**를 클릭하여 현재 Coastfile 구성에서 다시 생성합니다.

이렇게 하면 공유 서비스 컨테이너가 제거된 후 다시 생성되며, 올바른 호스트 볼륨에 다시 연결됩니다.

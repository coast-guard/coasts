# 원격 코스트

> **베타.** 원격 코스트는 완전히 동작하지만 CLI 플래그, Coastfile 스키마, 그리고 coast-service API는 향후 릴리스에서 변경될 수 있습니다. 버그나 결함을 발견하면 pull request를 열거나 이슈를 등록해 주세요.

원격 코스트는 서비스들을 원격 머신에서 실행하면서도 개발자 경험은 로컬 코스트와 동일하게 유지합니다. `coast run`, `coast assign`, `coast exec`, `coast ps`, `coast logs`, 그리고 그 외 모든 명령은 동일한 방식으로 동작합니다. 데몬은 인스턴스가 원격임을 감지하고 SSH 터널을 통해 작업을 투명하게 라우팅합니다.

## 왜 원격인가

로컬 코스트는 모든 것을 여러분의 노트북에서 실행합니다. 각 코스트 인스턴스는 전체 compose 스택(웹 서버, API, 워커, 데이터베이스, 캐시, 메일 서버)을 포함한 완전한 Docker-in-Docker 컨테이너를 실행합니다. 이는 노트북의 RAM이나 디스크 공간이 부족해질 때까지는 잘 동작합니다.

여러 서비스가 있는 풀스택 프로젝트는 코스트 하나당 상당한 RAM을 소비할 수 있습니다. 몇 개의 코스트를 병렬로 실행하면 곧 노트북의 한계에 도달하게 됩니다.

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

  Total: 48 GB RAM on your laptop
```

원격 코스트를 사용하면 일부 코스트를 원격 머신으로 옮겨 수평 확장할 수 있습니다. DinD 컨테이너, compose 서비스, 이미지 빌드는 원격에서 실행되고, 에디터와 에이전트는 로컬에 남아 있습니다. Postgres와 Redis 같은 공유 서비스도 로컬에 유지되며, SSH 역방향 터널을 통해 로컬 및 원격 인스턴스 간 데이터베이스 동기화를 유지합니다.

```text
  Your Machine                         Remote Server
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agents    │             │  coast-1 (all services) │
  │                     │  SSH        │  coast-2 (all services) │
  │  shared services    │──tunnels──▶ │  coast-3 (all services) │
  │  (postgres, redis)  │             │  coast-4 (all services) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: lightweight                  Server: 64 GB RAM, 16 CPU
```

로컬호스트 런타임을 수평 확장하세요.

## 빠른 시작

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

호스트 준비 및 coast-service 배포를 포함한 전체 설정 지침은 [Setup](SETUP.md)를 참조하세요.

## 참조

| Page | What it covers |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | 두 컨테이너 분할(셸 코스트 + 원격 코스트), SSH 터널 계층, 포트 포워딩 체인, 그리고 데몬이 요청을 라우팅하는 방식 |
| [Setup](SETUP.md) | 호스트 요구 사항, coast-service 배포, 원격 등록, 그리고 엔드투엔드 빠른 시작 |
| [File Sync](FILE_SYNC.md) | 대량 전송을 위한 rsync, 연속 동기화를 위한 mutagen, run/assign/stop 전반의 수명 주기, 제외 항목, 그리고 경쟁 상태 처리 |
| [Builds](BUILDS.md) | 네이티브 아키텍처를 위한 원격 빌드, 아티팩트 전송, `latest-remote` 심볼릭 링크, 아키텍처 재사용, 그리고 자동 정리 |
| [CLI and Configuration](CLI.md) | `coast remote` 명령, `Coastfile.remote` 구성, 디스크 관리, 그리고 `coast remote prune` |

## 함께 보기

- [Remotes](../concepts_and_terminology/REMOTES.md) -- 용어집의 개념 개요
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- 로컬 공유 서비스가 원격 코스트로 역방향 터널링되는 방식
- [Ports](../concepts_and_terminology/PORTS.md) -- SSH 터널 계층이 canonical/dynamic 포트 모델에 어떻게 들어맞는지

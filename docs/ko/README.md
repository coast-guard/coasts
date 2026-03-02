# Coasts 문서

## 설치

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install`을 실행하지 않기로 결정한다면, 매번 반드시 `coast daemon start`로 데몬을 수동으로 시작할 책임은 사용자에게 있습니다.*

## Coasts란?

Coast(**컨테이너화된 호스트**)는 로컬 개발 런타임입니다. Coasts를 사용하면 한 대의 머신에서 동일한 프로젝트에 대해 여러 개의 격리된 환경을 실행할 수 있습니다.

Coasts는 특히 많은 상호의존 서비스가 있는 복잡한 `docker-compose` 스택에 유용하지만, 컨테이너화되지 않은 로컬 개발 설정에서도 동일하게 효과적입니다. Coasts는 다양한 [런타임 구성 패턴](concepts_and_terminology/RUNTIMES_AND_SERVICES.md)을 지원하므로, 병렬로 작업하는 여러 에이전트에 맞춰 이상적인 환경을 구성할 수 있습니다.

Coasts는 호스팅된 클라우드 서비스가 아니라 로컬 개발을 위해 만들어졌습니다. 환경은 사용자의 머신에서 로컬로 실행됩니다.

Coasts 프로젝트는 무료이며 로컬에서 동작하고 MIT 라이선스를 따르며, 에이전트 제공자에 구애받지 않고, 에이전트 하니스에 구애받지 않는 소프트웨어로 AI 업셀링이 없습니다.

Coasts는 worktree를 사용하는 어떤 에이전틱 코딩 워크플로와도 함께 동작합니다. 하니스 측의 특별한 설정은 필요하지 않습니다.

## Worktrees에 Coasts가 필요한 이유

Git worktree는 코드 변경을 격리하는 데 매우 훌륭하지만, 그것만으로는 런타임 격리를 해결하지 못합니다.

여러 worktree를 병렬로 실행하면, 금방 사용성(ergonomics) 문제가 발생합니다:

- 동일한 호스트 포트를 기대하는 서비스들 사이의 [포트 충돌](concepts_and_terminology/PORTS.md).
- worktree별 데이터베이스 및 [볼륨 설정](concepts_and_terminology/VOLUMES.md)은 관리하기 번거롭습니다.
- worktree별로 커스텀 런타임 배선이 필요한 통합 테스트 환경.
- worktree를 전환할 때마다 런타임 컨텍스트를 다시 구축해야 하는 생지옥. [Assign and Unassign](concepts_and_terminology/ASSIGN.md)을 참고하세요.

Git이 코드에 대한 버전 관리라면, Coasts는 worktree 런타임에 대한 Git과 같습니다.

각 환경은 고유한 포트를 가지므로, 어떤 worktree 런타임이든 병렬로 확인할 수 있습니다. worktree 런타임을 [체크아웃](concepts_and_terminology/CHECKOUT.md)하면, Coasts가 해당 런타임을 프로젝트의 표준 포트로 다시 매핑합니다.

Coasts는 worktree 위에 단순한 모듈형 레이어로 런타임 구성을 추상화하여, 각 worktree가 복잡한 worktree별 설정을 손으로 유지보수하지 않고도 필요한 격리 수준으로 실행될 수 있게 합니다.

## 요구 사항

- macOS
- Docker Desktop
- Git을 사용하는 프로젝트
- Node.js
- `socat` *(`curl -fsSL https://coasts.dev/install | sh`로 설치 시 Homebrew `depends_on` 의존성으로 설치됨)*

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## 에이전트를 컨테이너화하나요?

Coast로 에이전트를 컨테이너화할 수 있습니다. 처음에는 매우 좋은 아이디어처럼 들릴 수 있지만, 많은 경우 실제로는 코딩 에이전트를 컨테이너 안에서 실행할 필요가 없습니다.

Coasts는 공유 볼륨 마운트를 통해 호스트 머신과 [파일시스템](concepts_and_terminology/FILESYSTEM.md)을 공유하므로, 가장 쉽고 신뢰할 수 있는 워크플로는 에이전트를 호스트에서 실행하고, 통합 테스트 같은 런타임 부하가 큰 작업을 Coast 인스턴스 내부에서 [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md)로 실행하도록 지시하는 것입니다.

하지만 에이전트를 컨테이너에서 실행하고 싶다면, Coasts는 [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md)를 통해 이를 확실히 지원합니다. [MCP 서버 구성](concepts_and_terminology/MCP_SERVERS.md)을 포함해 이 설정을 위한 매우 정교한 리그를 구축할 수 있지만, 오늘날 존재하는 오케스트레이션 소프트웨어와 깔끔하게 상호운용되지 않을 수 있습니다. 대부분의 워크플로에서는 호스트 측 에이전트가 더 단순하고 더 신뢰할 수 있습니다.

## Coasts vs Dev Containers

Coasts는 dev container가 아니며, 같은 것도 아닙니다.

Dev container는 일반적으로 IDE를 단일 컨테이너화된 개발 워크스페이스에 마운트하도록 설계됩니다. Coasts는 헤드리스이며, worktree와 함께 병렬로 사용하는 에이전트를 위한 경량 환경으로 최적화되어 있습니다 — 여러 개의 격리된, worktree 인지형 런타임 환경을 나란히 실행하고, 빠른 체크아웃 전환과 인스턴스별 런타임 격리 제어를 제공합니다.

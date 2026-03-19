# Coasts 문서

```youtube
MBGKSKau4sU
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## 설치

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install`을 실행하지 않기로 결정한 경우, 매번 `coast daemon start`로 데몬을 수동으로 시작할 책임은 사용자에게 있습니다.*

## Coasts란?

A Coast(**컨테이너화된 호스트**)는 로컬 개발 런타임입니다. Coasts를 사용하면 한 대의 머신에서 동일한 프로젝트에 대해 여러 개의 격리된 환경을 실행할 수 있습니다.

Coasts는 특히 많은 상호의존 서비스가 있는 복잡한 `docker-compose` 스택에 유용하지만, 컨테이너화되지 않은 로컬 개발 셋업에서도 동일하게 효과적입니다. Coasts는 광범위한 [런타임 구성 패턴](concepts_and_terminology/RUNTIMES_AND_SERVICES.md)을 지원하므로, 병렬로 작업하는 여러 에이전트에 대해 이상적인 환경을 구성할 수 있습니다.

Coasts는 호스팅 클라우드 서비스가 아니라 로컬 개발을 위해 만들어졌습니다. 여러분의 환경은 여러분의 머신에서 로컬로 실행됩니다.

Coasts 프로젝트는 무료이며 로컬에서 동작하고, MIT 라이선스를 따르며, 에이전트 제공자에 종속되지 않고, 에이전트 하네스에도 종속되지 않는 소프트웨어로, AI 업셀링이 없습니다.

Coasts는 worktree를 사용하는 어떤 에이전틱 코딩 워크플로우와도 함께 동작합니다. 하네스 측의 특별한 구성은 필요하지 않습니다.

## Worktrees를 위한 Coasts를 사용하는 이유

Git worktree는 코드 변경을 격리하는 데 훌륭하지만, 런타임 격리를 그 자체로 해결하지는 못합니다.

여러 worktree를 병렬로 실행하면, 금방 사용성 문제에 부딪히게 됩니다:

- 동일한 호스트 포트를 기대하는 서비스들 간의 [포트 충돌](concepts_and_terminology/PORTS.md).
- 관리가 번거로운 worktree별 데이터베이스 및 [볼륨 설정](concepts_and_terminology/VOLUMES.md).
- worktree마다 커스텀 런타임 배선이 필요한 통합 테스트 환경.
- worktree를 전환할 때마다 런타임 컨텍스트를 다시 빌드해야 하는, 말 그대로 살아있는 지옥. [할당 및 할당 해제](concepts_and_terminology/ASSIGN.md)를 참고하세요.

Git이 코드에 대한 버전 관리라면, Coasts는 worktree 런타임에 대한 Git과 같습니다.

각 환경은 자체 포트를 가지므로, 어떤 worktree 런타임이든 병렬로 검사할 수 있습니다. worktree 런타임을 [체크아웃](concepts_and_terminology/CHECKOUT.md)하면, Coasts는 해당 런타임을 프로젝트의 표준 포트로 리매핑합니다.

Coasts는 런타임 구성을 worktree 위에 얹는 간단한 모듈식 레이어로 추상화하여, 각 worktree가 복잡한 worktree별 셋업을 손으로 유지보수하지 않고도 필요한 격리 수준으로 실행될 수 있게 합니다.

## 요구 사항

- macOS 또는 Linux
- macOS에서는 Docker Desktop, Linux에서는 Compose 플러그인이 포함된 Docker Engine
- Git을 사용하는 프로젝트
- Node.js
- `socat` (macOS에서는 `brew install socat`, Ubuntu에서는 `sudo apt install socat`)

```text
Linux 참고: 동적 포트는 Linux에서 별도 설정 없이 바로 동작합니다.
`1024` 미만의 표준 포트가 필요하다면, 필요한 호스트 구성을 위해 checkout 문서를 참고하세요.
```

## 에이전트를 컨테이너화할까요?

Coast로 에이전트를 컨테이너화할 수 있습니다. 처음에는 아주 좋은 아이디어처럼 들릴 수 있지만, 많은 경우 실제로는 코딩 에이전트를 컨테이너 안에서 실행할 필요가 없습니다.

Coasts는 공유 볼륨 마운트를 통해 호스트 머신과 [파일시스템](concepts_and_terminology/FILESYSTEM.md)을 공유하기 때문에, 가장 쉽고 신뢰할 수 있는 워크플로우는 에이전트를 호스트에서 실행하고 [`coast exec`](concepts_and_terminology/EXEC_AND_DOCKER.md)를 사용해 통합 테스트 같은 런타임 부담이 큰 작업을 Coast 인스턴스 내부에서 실행하도록 지시하는 것입니다.

하지만 에이전트를 컨테이너에서 실행하고 싶다면, Coasts는 [Agent Shells](concepts_and_terminology/AGENT_SHELLS.md)를 통해 이를 확실히 지원합니다. [MCP 서버 구성](concepts_and_terminology/MCP_SERVERS.md)을 포함해 이 셋업을 위한 믿을 수 없을 정도로 정교한 리그를 구축할 수도 있지만, 현재 존재하는 오케스트레이션 소프트웨어와 깔끔하게 상호운용되지 않을 수 있습니다. 대부분의 워크플로우에서는 호스트 측 에이전트가 더 단순하고 신뢰할 수 있습니다.

## Coasts vs Dev Containers

Coasts는 dev container가 아니며, 같은 것도 아닙니다.

Dev container는 일반적으로 IDE를 단일 컨테이너화된 개발 워크스페이스에 마운트하도록 설계됩니다. Coasts는 헤드리스이며, worktree를 사용하는 병렬 에이전트 사용을 위한 경량 환경으로 최적화되어 있습니다 — 여러 개의 격리된, worktree 인지형 런타임 환경이 나란히 실행되며, 빠른 체크아웃 전환과 각 인스턴스별 런타임 격리 제어를 제공합니다.

## Demo Repo

Coasts로 시험해 볼 수 있는 작은 예제 프로젝트를 원한다면, [`coasts-demo` repository](https://github.com/coast-guard/coasts-demo)로 시작하세요.

## Coasts Video Course

비디오를 선호한다면, [Coasts Video Course](learn-coasts-videos/README.md)에서 각 핵심 개념을 3분 이내로 모두 다룹니다.

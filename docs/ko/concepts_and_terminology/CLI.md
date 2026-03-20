# Coast CLI

Coast CLI(`coast`)는 Coast를 운영하기 위한 기본 명령줄 인터페이스입니다. 의도적으로 얇게 설계되어 있습니다: 명령을 파싱하고, [`coastd`](DAEMON.md)에 요청을 보내며, 구조화된 출력을 터미널에 다시 출력합니다.

## 무엇에 사용하나요

일반적인 워크플로는 모두 CLI에서 구동됩니다:

```bash
coast build                                    # see Builds
coast run dev-1                                # see Run
coast assign dev-1 --worktree feature/oauth    # see Assign
coast ports dev-1                              # see Ports
coast checkout dev-1                           # see Checkout
coast ui                                       # see Coastguard
```

CLI에는 사람과 에이전트 모두에게 유용한 문서 명령도 포함되어 있습니다:

```bash
coast docs
coast docs --path concepts_and_terminology/CHECKOUT.md
coast search-docs "canonical vs dynamic ports"
```

## 데몬과 별도로 존재하는 이유

CLI를 데몬과 분리하면 몇 가지 중요한 이점이 있습니다:

- 데몬은 상태와 장시간 실행되는 프로세스를 유지합니다.
- CLI는 빠르고, 조합 가능하며, 스크립트 작성이 쉽습니다.
- 터미널 상태를 계속 유지하지 않고도 일회성 명령을 실행할 수 있습니다.
- 에이전트 도구는 예측 가능하고 자동화에 친화적인 방식으로 CLI 명령을 호출할 수 있습니다.

## CLI vs Coastguard

그 순간에 맞는 인터페이스를 사용하세요:

- CLI는 전체 운영 범위를 포괄하도록 설계되었습니다: Coastguard에서 할 수 있는 모든 일은 CLI에서도 가능해야 합니다.
- CLI는 자동화 인터페이스로 취급하세요 — 스크립트, 에이전트 워크플로, CI 작업, 그리고 맞춤형 개발자 도구를 위한 것입니다.
- [Coastguard](COASTGUARD.md)는 사람을 위한 인터페이스로 취급하세요 — 시각적 점검, 대화형 디버깅, 운영 가시성을 위한 것입니다.

둘 다 동일한 데몬과 통신하므로, 동일한 기반 프로젝트 상태에서 동작합니다.

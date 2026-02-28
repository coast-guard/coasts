# Coast 데몬

Coast 데몬(`coastd`)은 실제 오케스트레이션 작업을 수행하는 장시간 실행되는 로컬 프로세스입니다. [CLI](CLI.md)와 [Coastguard](COASTGUARD.md)는 클라이언트이며, `coastd`는 그 뒤에 있는 제어 평면입니다.

## 아키텍처 한눈에 보기

```text
coast CLI (automation) -----+
                            +--> coastd daemon
Coastguard UI (human) ------+         |
                                      +--> Coasts
                                      +--> Ports
                                      +--> State
```

CLI는 로컬 Unix 소켓을 통해 `coastd`로 요청을 보내며, Coastguard는 WebSocket을 통해 연결합니다. 데몬은 런타임 상태에 변경 사항을 적용합니다.

## 하는 일

`coastd`는 영속적인 상태와 백그라운드 조정이 필요한 작업을 처리합니다:

- Coast 인스턴스, 빌드, 공유 서비스를 추적합니다.
- Coast 런타임을 생성, 시작, 중지, 제거합니다.
- 할당/할당 해제/체크아웃 작업을 적용합니다.
- 정적 및 동적 [포트 포워딩](PORTS.md)을 관리합니다.
- [로그](LOGS.md), 상태, 런타임 이벤트를 CLI 및 UI 클라이언트로 스트리밍합니다.

요약하면: `coast run`, `coast assign`, `coast checkout`, `coast ls`를 실행할 때 실제로 작업을 수행하는 구성 요소는 데몬입니다.

## 실행 방법

데몬을 실행하는 일반적인 방법은 두 가지입니다:

```bash
# Register daemon auto-start at login (recommended)
coast daemon install

# Manual start mode
coast daemon start
```

daemon install을 건너뛰면, Coast 명령을 사용하기 전에 매 세션마다 직접 시작해야 합니다.

## 버그 제보

문제가 발생하면 버그 리포트를 제출할 때 `coastd` 데몬 로그를 포함해 주세요. 로그에는 대부분의 문제를 진단하는 데 필요한 컨텍스트가 담겨 있습니다:

```bash
coast daemon logs
```

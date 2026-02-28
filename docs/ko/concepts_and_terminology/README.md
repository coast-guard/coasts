# 개념과 용어

이 섹션에서는 Coasts 전반에서 사용되는 핵심 개념과 용어를 다룹니다. Coasts가 처음이라면, 설정이나 고급 사용법으로 들어가기 전에 여기서 시작하세요.

- [Coasts](COASTS.md) — 프로젝트의 자체 완결형 런타임으로, 각각 고유한 포트, 볼륨, 워크트리 할당을 가집니다.
- [파일시스템](FILESYSTEM.md) — 호스트와 Coast 사이의 공유 마운트, 호스트 측 에이전트, 그리고 워크트리 전환.
- [Coast 데몬](DAEMON.md) — 라이프사이클 작업을 실행하는 로컬 `coastd` 컨트롤 플레인.
- [Coast CLI](CLI.md) — 명령, 스크립트, 에이전트 워크플로를 위한 터미널 인터페이스.
- [Coastguard](COASTGUARD.md) — 관측성과 제어를 위해 `coast ui`로 실행되는 웹 UI.
- [포트](PORTS.md) — 표준 포트 vs 동적 포트, 그리고 체크아웃이 이들 사이를 어떻게 교체하는지.
- [기본 포트 & DNS](PRIMARY_PORT_AND_DNS.md) — 기본 서비스로의 빠른 링크, 쿠키 격리를 위한 서브도메인 라우팅, 그리고 URL 템플릿.
- [할당 및 할당 해제](ASSIGN.md) — Coast를 워크트리 간에 전환하는 방법과 사용 가능한 할당 전략.
- [체크아웃](CHECKOUT.md) — 표준 포트를 Coast 인스턴스에 매핑하는 것과 언제 필요한지.
- [조회](LOOKUP.md) — 에이전트의 현재 워크트리와 일치하는 Coast 인스턴스를 찾기.
- [볼륨 토폴로지](VOLUMES.md) — 공유 서비스, 공유 볼륨, 격리된 볼륨, 그리고 스냅샷.
- [공유 서비스](SHARED_SERVICES.md) — 호스트가 관리하는 인프라 서비스와 볼륨 구분.
- [시크릿과 추출기](SECRETS.md) — 호스트 시크릿을 추출하고 이를 Coast 컨테이너에 주입하기.
- [빌드](BUILDS.md) — coast 빌드의 구성, 아티팩트가 저장되는 위치, 자동 정리, 그리고 타입이 지정된 빌드.
- [Coastfile 타입](COASTFILE_TYPES.md) — extends, unset, omit, autostart를 통한 조합 가능한 Coastfile 변형.
- [런타임과 서비스](RUNTIMES_AND_SERVICES.md) — DinD 런타임, Docker-in-Docker 아키텍처, 그리고 서비스가 Coast 내부에서 실행되는 방식.
- [베어 서비스](BARE_SERVICES.md) — Coast 내부에서 컨테이너화되지 않은 프로세스를 실행하는 방법과 대신 컨테이너화해야 하는 이유.
- [로그](LOGS.md) — Coast 내부에서 서비스 로그를 읽는 방법, MCP 트레이드오프, 그리고 Coastguard 로그 뷰어.
- [Exec & Docker](EXEC_AND_DOCKER.md) — Coast 내부에서 명령을 실행하고 내부 Docker 데몬과 통신하기.
- [에이전트 셸](AGENT_SHELLS.md) — 컨테이너화된 에이전트 TUI, OAuth 트레이드오프, 그리고 아마도 호스트에서 에이전트를 실행해야 하는 이유.
- [MCP 서버](MCP_SERVERS.md) — 컨테이너화된 에이전트를 위해 Coast 내부에서 MCP 도구를 구성하기, 내부 서버 vs 호스트 프록시 서버.

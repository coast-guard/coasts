# 개념과 용어

이 섹션에서는 Coasts 전반에서 사용되는 핵심 개념과 용어를 다룹니다. Coasts가 처음이라면 구성이나 고급 사용법으로 들어가기 전에 여기부터 시작하세요.

- [Coasts](COASTS.md) - 각각 고유한 포트, 볼륨, 워크트리 할당을 가지는, 프로젝트의 자체 포함 런타임입니다.
- [Run](RUN.md) - 최신 빌드에서 새로운 Coast 인스턴스를 생성하고, 선택적으로 워크트리를 할당하는 것입니다.
- [Remove](REMOVE.md) - 깨끗하게 다시 생성해야 하거나 Coasts를 중지하려는 경우, Coast 인스턴스와 그 격리된 런타임 상태를 해체하는 것입니다.
- [Filesystem](FILESYSTEM.md) - 호스트와 Coast 간의 공유 마운트, 호스트 측 에이전트, 워크트리 전환을 다룹니다.
- [Private Paths](PRIVATE_PATHS.md) - 공유 바인드 마운트 전반에서 충돌하는 워크스페이스 경로에 대한 인스턴스별 격리입니다.
- [Coast Daemon](DAEMON.md) - 라이프사이클 작업을 실행하는 로컬 `coastd` 제어 플레인입니다.
- [Coast CLI](CLI.md) - 명령, 스크립트, 에이전트 워크플로를 위한 터미널 인터페이스입니다.
- [Coastguard](COASTGUARD.md) - 관찰 가능성과 제어를 위해 `coast ui`로 실행되는 웹 UI입니다.
- [Ports](PORTS.md) - 표준 포트와 동적 포트, 그리고 checkout이 이들 사이를 어떻게 전환하는지 설명합니다.
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - 기본 서비스로의 빠른 링크, 쿠키 격리를 위한 서브도메인 라우팅, URL 템플릿을 다룹니다.
- [Assign and Unassign](ASSIGN.md) - Coast를 워크트리 간에 전환하는 방법과 사용 가능한 assign 전략입니다.
- [Checkout](CHECKOUT.md) - 표준 포트를 Coast 인스턴스에 매핑하는 것과, 그것이 필요한 경우를 설명합니다.
- [Lookup](LOOKUP.md) - 에이전트의 현재 워크트리와 일치하는 Coast 인스턴스를 찾는 것입니다.
- [Volume Topology](VOLUMES.md) - 공유 서비스, 공유 볼륨, 격리된 볼륨, 스냅샷을 다룹니다.
- [Shared Services](SHARED_SERVICES.md) - 호스트에서 관리되는 인프라 서비스와 볼륨 구분을 다룹니다.
- [Secrets and Extractors](SECRETS.md) - 호스트 시크릿을 추출하고 이를 Coast 컨테이너에 주입하는 방법입니다.
- [Builds](BUILDS.md) - coast 빌드의 구조, 아티팩트가 저장되는 위치, 자동 정리, 타입 지정 빌드를 다룹니다.
- [Coastfile Types](COASTFILE_TYPES.md) - extends, unset, omit, autostart를 갖는 조합 가능한 Coastfile 변형입니다.
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) - DinD 런타임, Docker-in-Docker 아키텍처, 서비스가 Coast 내부에서 실행되는 방식을 다룹니다.
- [Bare Services](BARE_SERVICES.md) - 비컨테이너화된 프로세스를 Coast 내부에서 실행하는 방법과, 대신 컨테이너화해야 하는 이유를 설명합니다.
- [Bare Service Optimization](BARE_SERVICE_OPTIMIZATION.md) - 조건부 설치, 캐싱, private_paths, 공유 서비스 연결성, bare 서비스용 assign 전략을 다룹니다.
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - 자동 주입되는 `<SERVICE>_DYNAMIC_PORT` 변수와 이를 서비스 명령에서 사용하는 방법을 설명합니다.
- [Logs](LOGS.md) - Coast 내부에서 서비스 로그를 읽는 방법, MCP 트레이드오프, Coastguard 로그 뷰어를 다룹니다.
- [Exec & Docker](EXEC_AND_DOCKER.md) - Coast 내부에서 명령을 실행하고 내부 Docker 데몬과 통신하는 방법입니다.
- [Agent Shells](AGENT_SHELLS.md) - 컨테이너화된 에이전트 TUI, OAuth 트레이드오프, 그리고 아마도 에이전트를 대신 호스트에서 실행해야 하는 이유를 설명합니다.
- [MCP Servers](MCP_SERVERS.md) - 컨테이너화된 에이전트를 위해 Coast 내부에서 MCP 도구를 구성하는 방법, 내부 서버와 호스트 프록시 서버의 차이를 다룹니다.
- [Remotes](REMOTES.md) - 로컬 워크플로는 변경하지 않은 채 coast-service를 통해 원격 머신에서 서비스를 실행하는 방법입니다.
- [Troubleshooting](TROUBLESHOOTING.md) - doctor, 데몬 재시작, 프로젝트 제거, 공장 초기화 수준의 nuke 옵션을 다룹니다.

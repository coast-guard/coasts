# Coastguard

Coastguard는 Coast의 로컬 웹 UI(예: Coast의 Docker Desktop 스타일 인터페이스)로, `31415` 포트에서 실행됩니다. CLI에서 실행합니다:

```bash
coast ui
```

![Coastguard project overview](../../assets/coastguard-overview.png)
*실행 중인 Coast 인스턴스, 해당 브랜치/워크트리, 그리고 체크아웃 상태를 보여주는 프로젝트 대시보드.*

![Coastguard port mappings](../../assets/coastguard-ports.png)
*특정 Coast 인스턴스의 포트 페이지로, 각 서비스에 대한 표준(canonical) 및 동적 포트 매핑을 보여줍니다.*

## Coastguard가 유용한 점

Coastguard는 프로젝트를 위한 시각적 제어 및 관측(Observability) 표면을 제공합니다:

- 프로젝트, 인스턴스, 상태, 브랜치, 체크아웃 상태를 확인합니다.
- [포트 매핑](PORTS.md)을 검사하고 서비스로 바로 이동합니다.
- [로그](LOGS.md), 런타임 통계, 그리고 데이터를 검사합니다.
- [빌드](BUILDS.md), 이미지 아티팩트, [볼륨](VOLUMES.md), [시크릿](SECRETS.md) 메타데이터를 탐색합니다.
- 작업 중 앱 내에서 문서를 탐색합니다.

## CLI 및 데몬과의 관계

Coastguard는 CLI를 대체하지 않습니다. 사람 중심의 인터페이스로서 이를 보완합니다.

- [`coast` CLI](CLI.md)는 스크립트, 에이전트 워크플로, 툴링 통합을 위한 자동화 인터페이스입니다.
- Coastguard는 시각적 점검, 대화형 디버깅, 일상적인 운영 가시성을 위한 사람 중심의 인터페이스입니다.
- 둘 다 [`coastd`](DAEMON.md)의 클라이언트이므로 동기화를 유지합니다.

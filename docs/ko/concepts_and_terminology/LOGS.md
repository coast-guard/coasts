# 로그

Coast 내부의 서비스는 중첩 컨테이너에서 실행됩니다 — compose 서비스는 DinD 컨테이너 내부의 내부 Docker 데몬에 의해 관리됩니다. 이는 호스트 수준 로깅 도구가 이를 볼 수 없다는 뜻입니다. 워크플로에 호스트에서 Docker 로그를 읽는 로깅 MCP가 포함되어 있다면, 이는 외부 DinD 컨테이너만 보게 되며 그 안에서 실행되는 웹 서버, 데이터베이스, 워커는 보지 못합니다.

해결책은 `coast logs`입니다. Coast 인스턴스에서 서비스 출력을 읽어야 하는 모든 에이전트나 도구는 호스트 수준 Docker 로그 접근 대신 Coast CLI를 사용해야 합니다.

## MCP의 트레이드오프

로깅 MCP(호스트에서 Docker 컨테이너 로그를 캡처하는 도구 — [MCP Servers](MCP_SERVERS.md) 참조)를 사용하는 AI 에이전트를 사용 중이라면, 해당 MCP는 Coast 내부에서 실행되는 서비스에 대해 작동하지 않습니다. 호스트 Docker 데몬은 Coast 인스턴스당 하나의 컨테이너 — DinD 컨테이너 — 만 보며, 그 로그는 내부 Docker 데몬의 시작 출력일 뿐입니다.

실제 서비스 로그를 캡처하려면, 에이전트가 다음을 사용하도록 지시하세요:

```bash
coast logs <instance> --service <service> --tail <lines>
```

예를 들어, 에이전트가 백엔드 서비스가 실패하는 이유를 점검해야 한다면:

```bash
coast logs dev-1 --service backend --tail 100
```

이는 `docker compose logs`와 동등하지만, Coast 데몬을 통해 내부 DinD 컨테이너로 라우팅됩니다. 로깅 MCP를 참조하는 에이전트 규칙이나 시스템 프롬프트가 있다면, Coast 내부에서 작업할 때 이 동작을 재정의하는 지시를 추가해야 합니다.

## `coast logs`

CLI는 Coast 인스턴스의 로그를 읽는 여러 가지 방법을 제공합니다:

```bash
coast logs dev-1                           # last 200 lines, all services
coast logs dev-1 --service web             # last 200 lines, web only
coast logs dev-1 --tail 50                 # last 50 lines, then follow
coast logs dev-1 --tail                    # all lines, then follow
coast logs dev-1 --service backend -f      # follow mode (stream new entries)
coast logs dev-1 --service web --tail 100  # last 100 lines + follow
```

`--tail` 또는 `-f` 없이 실행하면, 명령은 마지막 200줄을 반환하고 종료합니다. `--tail`을 사용하면 요청한 줄 수를 스트리밍한 다음, 새 출력이 생성되는 대로 실시간으로 계속 따라갑니다. `-f` / `--follow`는 단독으로 follow 모드를 활성화합니다.

출력은 각 줄에 서비스 접두사가 붙는 compose 로그 형식을 사용합니다:

```text
web       | 2026/02/28 01:49:34 Listening on :3000
backend   | 2026/02/28 01:49:34 [INFO] Server started on :8080
backend   | 2026/02/28 01:49:34 [ProcessCreditsJob] starting at 2026-02-28T01:49:34Z
redis     | 1:M 28 Feb 2026 01:49:30.123 * Ready to accept connections
```

레거시 위치 기반 문법(`coast logs dev-1 web`)으로도 서비스를 기준으로 필터링할 수 있지만, `--service` 플래그 사용을 권장합니다.

## Coastguard 로그 탭

Coastguard 웹 UI는 WebSocket을 통한 실시간 스트리밍으로 더 풍부한 로그 뷰잉 경험을 제공합니다.

![Logs tab in Coastguard](../../assets/coastguard-logs.png)
*서비스 필터링과 검색을 사용해 백엔드 서비스 출력을 스트리밍하는 Coastguard 로그 탭.*

로그 탭은 다음을 제공합니다:

- **실시간 스트리밍** — 로그는 생성되는 대로 WebSocket 연결을 통해 도착하며, 연결 상태를 보여주는 상태 표시기가 있습니다.
- **서비스 필터** — 로그 스트림의 서비스 접두사로 채워지는 드롭다운입니다. 단일 서비스를 선택해 해당 출력에 집중할 수 있습니다.
- **검색** — 텍스트 또는 정규식으로 표시된 줄을 필터링합니다(정규식 모드는 별표 버튼을 토글). 일치하는 용어는 하이라이트됩니다.
- **줄 수** — 필터된 줄 수 대비 전체 줄 수를 표시합니다(예: "200 / 971 lines").
- **지우기** — 내부 컨테이너 로그 파일을 잘라내고 뷰어를 초기화합니다.
- **전체 화면** — 로그 뷰어를 화면 전체로 확장합니다.

로그 라인은 ANSI 색상 지원, 로그 레벨 하이라이팅(ERROR는 빨강, WARN은 호박색, INFO는 파랑, DEBUG는 회색), 타임스탬프 디밍, 그리고 서비스 간 시각적 구분을 위한 컬러 서비스 배지로 렌더링됩니다.

호스트 데몬에서 실행되는 공유 서비스는 Shared Services 탭에서 접근 가능한 자체 로그 뷰어를 가집니다. 자세한 내용은 [Shared Services](SHARED_SERVICES.md)를 참고하세요.

## 동작 방식

`coast logs`를 실행하면, 데몬은 `docker exec`를 통해 DinD 컨테이너 내부에서 `docker compose logs`를 실행하고 출력을 터미널(또는 WebSocket을 통해 Coastguard UI)로 스트리밍합니다.

```text
coast logs dev-1 --service web --tail 50
  │
  ├── CLI sends LogsRequest to daemon (Unix socket)
  │
  ├── Daemon resolves instance → container ID
  │
  ├── Daemon exec's into DinD container:
  │     docker compose logs --tail 50 --follow web
  │
  └── Output streams back chunk by chunk
        └── CLI prints to stdout / Coastguard renders in UI
```

[bare services](BARE_SERVICES.md)의 경우, 데몬은 `docker compose logs`를 호출하는 대신 `/var/log/coast-services/`의 로그 파일을 tail 합니다. 출력 형식은 동일하며(`service  | line`), 따라서 두 경우 모두 서비스 필터링이 동일하게 작동합니다.

## 관련 명령

- `coast ps <instance>` — 어떤 서비스가 실행 중인지와 상태를 확인합니다. [Runtimes and Services](RUNTIMES_AND_SERVICES.md)를 참고하세요.
- [`coast exec <instance>`](EXEC_AND_DOCKER.md) — 수동 디버깅을 위해 Coast 컨테이너 내부에서 셸을 엽니다.

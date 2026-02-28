# MCP 서버

MCP(Model Context Protocol) 서버는 AI 에이전트가 도구에 접근할 수 있게 해줍니다 — 파일 검색, 데이터베이스 쿼리, 문서 조회, 브라우저 자동화 등. Coast는 Coast 컨테이너 내부에 MCP 서버를 설치하고 구성하여, 컨테이너화된 에이전트가 필요한 도구에 접근할 수 있도록 합니다.

**이는 에이전트를 Coast 컨테이너 내부에서 실행하는 경우에만 해당됩니다.** 에이전트를 호스트에서 실행한다면(권장 접근 방식) MCP 서버도 호스트에서 실행되며 이러한 구성은 필요하지 않습니다. 이 페이지는 [Agent Shells](AGENT_SHELLS.md)를 바탕으로 하며 그 위에 한 층의 복잡성을 더합니다. 진행하기 전에 해당 문서의 경고를 읽으세요.

## 내부 서버 vs 호스트-프록시 서버

Coast는 MCP 서버에 대해 두 가지 모드를 지원하며, 이는 Coastfile의 `[mcp]` 섹션에 있는 `proxy` 필드로 제어됩니다.

### 내부 서버

내부 서버는 `/mcp/<name>/`에 있는 DinD 컨테이너 내부에 설치되고 실행됩니다. 컨테이너화된 파일시스템과 실행 중인 서비스에 직접 접근할 수 있습니다.

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

프로젝트의 소스 파일을 MCP 디렉터리로 복사할 수도 있습니다:

```toml
[mcp.my-custom-tool]
source = "tools/my-mcp-server"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
```

`source` 필드는 설정 중에 `/workspace/<path>/`의 파일을 `/mcp/<name>/`로 복사합니다. `install` 명령은 해당 디렉터리 안에서 실행됩니다. 이는 리포지토리 안에 있는 MCP 서버에 유용합니다.

### 호스트-프록시 서버

호스트-프록시 서버는 컨테이너 내부가 아니라 호스트 머신에서 실행됩니다. Coast는 `coast-mcp-proxy`를 사용해 컨테이너에서 호스트로 네트워크를 통해 MCP 요청을 포워딩하는 클라이언트 구성을 생성합니다.

```toml
[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]
```

호스트-프록시 서버에는 `install` 또는 `source` 필드를 둘 수 없습니다 — 호스트에 이미 준비되어 있다고 가정합니다. 브라우저 자동화나 호스트 파일시스템 도구처럼 호스트 수준 접근이 필요한 MCP 서버에는 이 모드를 사용하세요.

### 어떤 경우에 무엇을 사용하나

| 모드 | 실행 위치 | 적합한 용도 | 제한 사항 |
|---|---|---|---|
| 내부 | DinD 컨테이너 | 컨테이너 파일시스템 접근이 필요한 도구, 프로젝트 전용 도구 | Alpine Linux에서 설치 가능해야 함, `coast run` 시간 증가 |
| 호스트-프록시 | 호스트 머신 | 브라우저 자동화, 호스트 수준 도구, 대형 사전 설치 서버 | 컨테이너 파일시스템에 직접 접근 불가 |

## 클라이언트 커넥터

`[mcp_clients]` 섹션은 컨테이너 내부의 에이전트가 서버를 발견할 수 있도록, 생성된 MCP 서버 구성을 어디에 작성할지 Coast에 알려줍니다.

### 내장 형식

Claude Code와 Cursor의 경우, 올바른 이름으로 빈 섹션만 있어도 충분합니다 — Coast가 형식과 기본 구성 경로를 자동 감지합니다:

```toml
[mcp_clients.claude-code]
# Writes to /root/.claude/mcp_servers.json (auto-detected)

[mcp_clients.cursor]
# Writes to /workspace/.cursor/mcp.json (auto-detected)
```

### 사용자 지정 구성 경로

다른 AI 도구의 경우, 형식과 경로를 명시적으로 지정하세요:

```toml
[mcp_clients.my-tool]
format = "claude-code"
config_path = "/home/coast/.config/my-tool/mcp.json"
```

### 명령 기반 커넥터

파일을 작성하는 대신, 생성된 구성 JSON을 명령으로 파이프할 수 있습니다:

```toml
[mcp_clients.custom-setup]
run = "my-config-tool import-mcp --stdin"
```

`run` 필드는 `format` 및 `config_path`와 상호 배타적입니다.

## Coastguard MCP 탭

[Coastguard](COASTGUARD.md) 웹 UI는 MCP 탭에서 MCP 구성을 확인할 수 있는 가시성을 제공합니다.

![MCP tab in Coastguard](../../assets/coastguard-mcp.png)
*구성된 서버, 해당 도구, 클라이언트 구성 위치를 보여주는 Coastguard MCP 탭.*

이 탭에는 세 섹션이 있습니다:

- **MCP Servers** — 선언된 각 서버를 이름, 유형(내부 또는 호스트), 명령, 상태(Installed, Proxied, 또는 Not Installed)와 함께 나열합니다.
- **Tools** — 서버를 선택해 MCP 프로토콜을 통해 노출하는 도구를 검사합니다. 각 도구는 이름과 설명을 보여주며, 클릭하면 전체 입력 스키마를 볼 수 있습니다.
- **Client Locations** — 생성된 구성 파일이 어디에 작성되었는지 보여줍니다(예: `claude-code` 형식이 `/root/.claude/mcp_servers.json`에 작성됨).

## CLI 명령

```bash
coast mcp dev-1 ls                          # list servers with type and status
coast mcp dev-1 tools context7              # list tools exposed by a server
coast mcp dev-1 tools context7 info resolve # show input schema for a specific tool
coast mcp dev-1 locations                   # show where client configs were written
```

`tools` 명령은 컨테이너 내부의 MCP 서버 프로세스에 JSON-RPC `initialize` 및 `tools/list` 요청을 보내는 방식으로 동작합니다. 이는 내부 서버에서만 동작합니다 — 호스트-프록시 서버는 호스트에서 검사해야 합니다.

## 설치 동작 방식

`coast run` 동안, 내부 Docker 데몬이 준비되고 서비스가 시작된 뒤 Coast는 MCP를 설정합니다:

1. 각 **내부** MCP 서버에 대해:
   - DinD 컨테이너 내부에 `/mcp/<name>/`를 생성
   - `source`가 설정되어 있으면 `/workspace/<source>/`에서 `/mcp/<name>/`로 파일을 복사
   - `/mcp/<name>/` 내부에서 각 `install` 명령을 실행(예: `npm install -g @upstash/context7-mcp`)

2. 각 **클라이언트 커넥터**에 대해:
   - 적절한 형식(Claude Code 또는 Cursor)으로 JSON 구성을 생성
   - 내부 서버는 실제 `command`와 `args`를 사용하고 `cwd`를 `/mcp/<name>/`로 설정
   - 호스트-프록시 서버는 `coast-mcp-proxy`를 명령으로 사용하고 서버 이름을 인자로 전달
   - 구성을 대상 경로에 작성(또는 `run` 명령으로 파이프)

호스트-프록시 서버는 컨테이너 내부의 `coast-mcp-proxy`가 MCP 프로토콜 요청을 호스트 머신으로 다시 포워딩하는 것에 의존하며, 실제 MCP 서버 프로세스는 호스트에서 실행됩니다.

## 전체 예시

내부 문서 도구와 호스트-프록시 브라우저 도구를 설정하고 Claude Code에 연결하는 Coastfile:

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp.browser]
proxy = "host"
command = "npx"
args = ["@anthropic-ai/browser-mcp"]

[mcp_clients.claude-code]
```

`coast run` 후, 컨테이너 내부의 Claude Code는 MCP 구성에서 두 서버 모두를 확인합니다 — `/mcp/context7/`에서 로컬로 실행되는 `context7`와, 호스트로 프록시된 `browser`.

## 호스트에서 실행되는 에이전트

코딩 에이전트가 호스트 머신에서 실행된다면(권장 접근 방식) MCP 서버도 호스트에서 실행되며 Coast의 `[mcp]` 구성은 관여하지 않습니다. 하지만 한 가지 고려할 점이 있습니다: **Coast 내부의 데이터베이스나 서비스에 연결하는 MCP 서버는 올바른 포트를 알아야 합니다.**

서비스가 Coast 내부에서 실행될 때, 새 인스턴스를 실행할 때마다 바뀌는 동적 포트로 접근할 수 있습니다. 호스트에서 `localhost:5432`에 연결하는 데이터베이스 MCP는 [체크아웃된](CHECKOUT.md) Coast의 데이터베이스에만 도달할 수 있습니다 — 또는 체크아웃된 Coast가 없다면 아무것도 연결되지 않습니다. 체크아웃되지 않은 인스턴스의 경우, MCP를 [동적 포트](PORTS.md)(예: `localhost:55681`)를 사용하도록 재구성해야 합니다.

이를 해결하는 방법은 두 가지가 있습니다:

**공유 서비스를 사용하세요.** 데이터베이스가 [공유 서비스](SHARED_SERVICES.md)로 실행된다면, 호스트 Docker 데몬에서 표준 포트(`localhost:5432`)로 존재합니다. 모든 Coast 인스턴스가 브리지 네트워크를 통해 여기에 연결하며, 호스트 측 MCP도 늘 사용하던 동일한 포트로 동일한 데이터베이스에 연결합니다. 재구성이 필요 없고 동적 포트 탐지도 필요 없습니다. 이것이 가장 간단한 접근입니다.

**`coast exec` 또는 `coast docker`를 사용하세요.** 데이터베이스가 Coast 내부(격리된 볼륨)에서 실행된다면, 호스트 측 에이전트는 여전히 Coast를 통해 명령을 실행하여 쿼리할 수 있습니다([Exec & Docker](EXEC_AND_DOCKER.md) 참조):

```bash
coast exec dev-1 -- psql -h localhost -U myuser -d mydb -c "SELECT count(*) FROM users"
coast docker dev-1 exec -i my-postgres psql -U myuser -d mydb -c "\\dt"
```

이 방식은 동적 포트를 전혀 알 필요가 없습니다 — 데이터베이스가 표준 포트로 존재하는 Coast 내부에서 명령이 실행되기 때문입니다.

대부분의 워크플로에서는 공유 서비스가 가장 저항이 적은 경로입니다. 호스트 MCP 구성은 Coasts를 사용하기 전과 정확히 동일하게 유지됩니다.

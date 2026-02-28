# MCP 서버와 클라이언트

> **참고:** MCP 설정은 [`[agent_shell]`](AGENT_SHELL.md)을 통해 Coast 컨테이너 안에서 코딩 에이전트를 실행할 때만 관련이 있습니다. 에이전트가 호스트에서 실행되는 경우(더 일반적인 설정)에는 이미 자체 MCP 서버에 접근할 수 있으므로 Coast가 이를 설정할 필요가 없습니다.

`[mcp.*]` 섹션은 Coast 인스턴스 내부에서 실행되거나 Coast 인스턴스와 함께 실행되는 MCP(Model Context Protocol) 서버를 구성합니다. `[mcp_clients.*]` 섹션은 이러한 서버를 Claude Code나 Cursor 같은 코딩 에이전트에 연결하여, 에이전트가 서버를 자동으로 발견하고 사용할 수 있게 합니다.

MCP 서버가 런타임에 어떻게 설치되고, 프록시되며, 관리되는지에 대해서는 [MCP Servers](../concepts_and_terminology/MCP_SERVERS.md)를 참고하세요.

## MCP 서버 — `[mcp.*]`

각 MCP 서버는 `[mcp]` 아래의 이름 있는 TOML 섹션입니다. 두 가지 모드가 있습니다: **internal**(Coast 컨테이너 내부에서 실행)과 **host-proxied**(호스트에서 실행되고 Coast로 프록시됨)입니다.

### Internal MCP 서버

Internal 서버는 DinD 컨테이너 내부에 설치되어 실행됩니다. `proxy`가 없을 때는 `command` 필드가 필요합니다.

```toml
[mcp.echo]
command = "node"
args = ["server.js"]
```

필드:

- **`command`** (필수) — 실행할 실행 파일
- **`args`** — 명령에 전달되는 인자
- **`env`** — 서버 프로세스를 위한 환경 변수
- **`install`** — 서버 시작 전에 실행할 명령(문자열 또는 배열 허용)
- **`source`** — 컨테이너의 `/mcp/{name}/`로 복사할 호스트 디렉터리

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]
```

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

### Host-proxied MCP 서버

Host-proxied 서버는 호스트 머신에서 실행되며 `coast-mcp-proxy`를 통해 Coast 내부에서 사용할 수 있게 됩니다. 이 모드를 활성화하려면 `proxy = "host"`로 설정하세요.

```toml
[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }
```

`proxy = "host"`일 때:

- `command`, `args`, `env`는 선택 사항입니다 — 생략하면 서버는 호스트의 기존 MCP 설정에서 이름으로 해석됩니다.
- `install`과 `source`는 **허용되지 않습니다**(서버는 컨테이너가 아니라 호스트에서 실행됩니다).

추가 필드가 없는 host-proxied 서버는 호스트 설정에서 이름으로 서버를 조회합니다:

```toml
[mcp.host-lookup]
proxy = "host"
```

`proxy`의 유일하게 유효한 값은 `"host"`입니다.

### 여러 서버

원하는 만큼 MCP 서버를 정의할 수 있습니다:

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]

[mcp.host-lookup]
proxy = "host"
```

## MCP 클라이언트 — `[mcp_clients.*]`

MCP 클라이언트 커넥터는 코딩 에이전트가 읽는 설정 파일에 MCP 서버 설정을 Coast가 어떻게 작성할지 알려줍니다. 이를 통해 `[mcp.*]` 서버가 에이전트에 자동으로 연결됩니다.

### 내장 커넥터

두 가지 커넥터가 내장되어 있습니다: `claude-code`와 `cursor`. 이를 사용하려면 추가 필드가 필요 없습니다.

```toml
[mcp_clients.claude-code]
```

```toml
[mcp_clients.cursor]
```

내장 커넥터는 자동으로 다음을 알고 있습니다:

- **`claude-code`** — `/root/.claude/mcp_servers.json`에 작성
- **`cursor`** — `/workspace/.cursor/mcp.json`에 작성

설정 경로를 재정의할 수 있습니다:

```toml
[mcp_clients.claude-code]
config_path = "/custom/path/mcp_servers.json"
```

### 커스텀 커넥터

내장되어 있지 않은 에이전트의 경우, `run` 필드를 사용해 Coast가 MCP 서버를 등록하기 위해 실행할 셸 명령을 지정하세요:

```toml
[mcp_clients.my-agent]
run = "my-agent mcp register --stdin"
```

`run` 필드는 `format` 또는 `config_path`와 함께 사용할 수 없습니다.

### 커스텀 포맷 커넥터

에이전트가 Claude Code 또는 Cursor와 동일한 설정 파일 포맷을 사용하지만 경로가 다른 경우:

```toml
[mcp_clients.my-agent]
format = "claude-code"
config_path = "/home/agent/.config/mcp.json"
```

`format`은 `"claude-code"` 또는 `"cursor"`여야 합니다. `format`과 함께 내장되지 않은 이름을 사용할 때는 `config_path`가 필요합니다.

## 예시

### Claude Code에 연결된 Internal MCP 서버

```toml
[mcp.context7]
install = "npm install -g @upstash/context7-mcp"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
```

### Internal 서버가 있는 Host-proxied 서버

```toml
[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-echo]
proxy = "host"
command = "node"
args = ["mcp-echo/server.js"]
env = { MCP_MODE = "host" }

[mcp_clients.claude-code]
```

### 여러 클라이언트 커넥터

```toml
[mcp.my-tools]
command = "my-mcp-server"
args = ["--port", "3100"]

[mcp_clients.claude-code]
[mcp_clients.cursor]
```

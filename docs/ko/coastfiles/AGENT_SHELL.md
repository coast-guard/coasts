# 에이전트 셸

> **대부분의 워크플로우에서는 코딩 에이전트를 컨테이너화할 필요가 없습니다.** Coast는 호스트 머신과 [파일시스템](../concepts_and_terminology/FILESYSTEM.md)을 공유하므로, 가장 간단한 접근은 호스트에서 에이전트를 실행하고 통합 테스트 같은 런타임이 무거운 작업에는 [`coast exec`](../concepts_and_terminology/EXEC_AND_DOCKER.md)를 사용하는 것입니다. 에이전트 셸은 에이전트가 컨테이너 내부에서 실행되기를 특별히 원하는 경우를 위한 것입니다 — 예를 들어 내부 Docker 데몬에 직접 접근하게 하거나, 환경을 완전히 격리하기 위해서입니다.

`[agent_shell]` 섹션은 Claude Code 또는 Codex 같은 에이전트 TUI가 Coast 컨테이너 내부에서 실행되도록 구성합니다. 이 섹션이 존재하면, Coast는 인스턴스가 시작될 때 구성된 명령을 실행하는 영구 PTY 세션을 자동으로 생성합니다.

에이전트 셸이 어떻게 동작하는지 전체 그림 — 활성 에이전트 모델, 입력 전송, 라이프사이클 및 복구 — 은 [에이전트 셸](../concepts_and_terminology/AGENT_SHELLS.md)을 참고하세요.

## 구성

이 섹션에는 필수 필드가 하나 있습니다: `command`.

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

### `command` (필수)

에이전트 PTY에서 실행할 셸 명령입니다. 이는 일반적으로 `[coast.setup]`을 통해 설치한 코딩 에이전트 CLI입니다.

이 명령은 DinD 컨테이너 내부의 `/workspace`(프로젝트 루트)에서 실행됩니다. 이는 compose 서비스가 아닙니다 — compose 스택 또는 단독 서비스들과 나란히 실행되며, 그 안에서 실행되는 것이 아닙니다.

## 라이프사이클

- 에이전트 셸은 `coast run` 시 자동으로 생성됩니다.
- [Coastguard](../concepts_and_terminology/COASTGUARD.md)에서는 닫을 수 없는 영구적인 "Agent" 탭으로 표시됩니다.
- 에이전트 프로세스가 종료되면, Coast가 이를 다시 생성(respawn)할 수 있습니다.
- `coast agent-shell input`을 통해 실행 중인 에이전트 셸로 입력을 전송할 수 있습니다.

## 예시

### Claude Code

`[coast.setup]`에서 Claude Code를 설치하고, [secrets](SECRETS.md)를 통해 자격 증명을 구성한 다음, 에이전트 셸을 설정합니다:

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git", "bash"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "mkdir -p /root/.claude",
]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "cd /workspace; exec claude --dangerously-skip-permissions --effort high"
```

### 간단한 에이전트 셸

기능이 동작하는지 테스트하기 위한 최소 에이전트 셸:

```toml
[coast]
name = "test-agent"

[coast.setup]
packages = ["bash"]

[agent_shell]
command = "exec sh -c 'while true; do echo agent-heartbeat; sleep 5; done'"
```

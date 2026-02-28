# 에이전트 셸

에이전트 셸은 Coast 내부에 있는 셸로, 에이전트 TUI 런타임(Claude Code, Codex 또는 어떤 CLI 에이전트)이 직접 열리도록 합니다. Coastfile의 `[agent_shell]` 섹션으로 이를 구성하면, Coast가 DinD 컨테이너 내부에서 에이전트 프로세스를 스폰합니다.

**대부분의 사용 사례에서는 이렇게 하지 않는 것이 좋습니다.** 대신 호스트 머신에서 코딩 에이전트를 실행하세요. 공유 [파일시스템](FILESYSTEM.md) 덕분에 호스트 측 에이전트는 일반적으로 코드를 편집하면서도 런타임 정보를 위해 [`coast logs`](LOGS.md), [`coast exec`](EXEC_AND_DOCKER.md), [`coast ps`](RUNTIMES_AND_SERVICES.md)를 호출할 수 있습니다. 에이전트 셸은 자격 증명 마운트, OAuth 복잡성, 라이프사이클 복잡성을 추가하며, 에이전트 자체를 컨테이너화해야 하는 특정한 이유가 없는 한 필요하지 않습니다.

## OAuth 문제

Claude Code, Codex 또는 OAuth로 인증하는 유사 도구를 사용 중이라면, 토큰은 호스트 머신을 대상으로 발급되었습니다. 동일한 토큰을 Linux 컨테이너 내부(다른 사용자 에이전트, 다른 환경)에서 사용하면, 제공자가 이를 플래그 처리하거나 철회할 수 있습니다. 디버깅하기 어려운 간헐적 인증 실패가 발생하게 됩니다.

컨테이너화된 에이전트에는 API 키 기반 인증이 더 안전한 선택입니다. 키를 Coastfile의 [시크릿](SECRETS.md)으로 설정하고 컨테이너 환경에 주입하세요.

API 키가 선택지가 아니라면 OAuth 자격 증명을 Coast에 마운트할 수 있습니다(아래 구성 섹션 참고). 다만 불편이 따를 수 있습니다. macOS에서 `keychain` 시크릿 추출기를 사용해 OAuth 토큰을 가져오면, 매번 `coast build` 시 macOS 키체인 비밀번호를 묻게 됩니다. 이는 특히 자주 리빌드할 때 빌드 과정을 번거롭게 만듭니다. 키체인 프롬프트는 macOS 보안 요구사항이며 우회할 수 없습니다.

## 구성

Coastfile에 실행할 커맨드를 포함한 `[agent_shell]` 섹션을 추가하세요:

```toml
[agent_shell]
command = "claude --dangerously-skip-permissions"
```

이 커맨드는 DinD 컨테이너 내부의 `/workspace`에서 실행됩니다. Coast는 컨테이너 안에 `coast` 사용자를 생성하고, `/root/.claude/`의 자격 증명을 `/home/coast/.claude/`로 복사한 뒤, 해당 사용자로 커맨드를 실행합니다. 에이전트에 컨테이너로 마운트해야 하는 자격 증명이 필요하다면, 파일 주입을 사용하는 `[secrets]`( [Secrets and Extractors](SECRETS.md) 참고 )와 에이전트 CLI를 설치하기 위한 `[coast.setup]`를 사용하세요:

```toml
[coast.setup]
run = ["npm install -g @anthropic-ai/claude-code"]

[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[agent_shell]
command = "claude --dangerously-skip-permissions"
```

`[agent_shell]`이 구성되어 있으면, Coast는 인스턴스 시작 시 셸을 자동으로 스폰합니다. 이 구성은 `extends`를 통해 상속되며, [Coastfile type](COASTFILE_TYPES.md)별로 오버라이드할 수 있습니다.

## 활성 에이전트 모델

각 Coast 인스턴스는 여러 에이전트 셸을 가질 수 있지만, 한 번에 **활성(active)** 상태인 것은 하나뿐입니다. 활성 셸은 `--shell` ID를 지정하지 않는 커맨드의 기본 대상입니다.

```bash
coast agent-shell dev-1 ls

  SHELL  STATUS   ACTIVE
  1      running  ★
  2      running
```

활성 셸을 전환하세요:

```bash
coast agent-shell dev-1 activate 2
```

활성 셸은 닫을 수 없습니다 — 먼저 다른 셸을 활성화하세요. 이는 상호작용 중인 셸을 실수로 종료하는 것을 방지합니다.

Coastguard에서는 에이전트 셸이 Exec 패널의 탭으로 표시되며 활성/비활성 배지가 붙습니다. 탭을 클릭해 터미널을 보고, 드롭다운 메뉴를 사용해 활성화, 스폰, 종료할 수 있습니다.

![Agent shell in Coastguard](../../assets/coastguard-agent-shell.png)
*Coastguard의 Exec 탭에서 접근 가능한 Coast 인스턴스 내부에서 Claude Code를 실행 중인 에이전트 셸.*

## 입력 보내기

컨테이너화된 에이전트를 프로그래밍 방식으로 구동하는 주요 방법은 `coast agent-shell input`입니다:

```bash
coast agent-shell dev-1 input "fix the failing test in auth.test.ts"
```

이 명령은 활성 에이전트의 TUI에 텍스트를 쓰고 Enter를 누릅니다. 에이전트는 터미널에 직접 타이핑한 것처럼 이를 받습니다.

옵션:

- `--no-send` — Enter를 누르지 않고 텍스트만 씁니다. 부분 입력을 쌓거나 TUI 메뉴를 탐색할 때 유용합니다.
- `--shell <id>` — 활성 셸 대신 특정 셸을 대상으로 합니다.
- `--show-bytes` — 디버깅을 위해 전송되는 정확한 바이트를 출력합니다.

내부적으로 입력은 PTY 마스터 파일 디스크립터에 직접 기록됩니다. 일부 TUI 프레임워크가 빠른 입력을 받을 때 보이는 paste-mode 아티팩트를 피하기 위해, 텍스트와 Enter 키스트로크는 25ms 간격을 두고 두 번의 별도 write로 전송됩니다.

## 기타 명령

```bash
coast agent-shell dev-1 spawn              # 새 셸 생성
coast agent-shell dev-1 spawn --activate   # 생성 후 즉시 활성화
coast agent-shell dev-1 tty                # 활성 셸에 대화형 TTY로 붙기
coast agent-shell dev-1 tty --shell 2      # 특정 셸에 붙기
coast agent-shell dev-1 read-output        # 전체 스크롤백 버퍼 읽기
coast agent-shell dev-1 read-last-lines 50 # 출력의 마지막 50줄 읽기
coast agent-shell dev-1 session-status     # 셸 프로세스가 살아있는지 확인
```

`tty`는 라이브 대화형 세션을 제공합니다 — 에이전트의 TUI에 직접 입력할 수 있습니다. 표준 터미널 이스케이프 시퀀스로 분리(detach)하세요. `read-output`과 `read-last-lines`는 비대화형이며 텍스트를 반환하므로, 스크립팅과 자동화에 유용합니다.

## 라이프사이클 및 복구

에이전트 셸 세션은 Coastguard에서 페이지를 이동해도 유지됩니다. 탭에 다시 연결하면 스크롤백 버퍼(최대 512KB)가 재생됩니다.

`coast stop`으로 Coast 인스턴스를 중지하면, 모든 에이전트 셸 PTY 프로세스가 종료되고 데이터베이스 레코드가 정리됩니다. `[agent_shell]`이 구성되어 있으면 `coast start`는 새로운 에이전트 셸을 자동으로 스폰합니다.

데몬이 재시작된 뒤에는 이전에 실행 중이던 에이전트 셸이 죽은 것으로 표시됩니다. 시스템이 이를 자동으로 감지합니다 — 활성 셸이 죽어 있으면, 첫 번째로 살아있는 셸이 활성으로 승격됩니다. 살아있는 셸이 없다면 `coast agent-shell spawn --activate`로 새 셸을 스폰하세요.

## 이것이 필요한 대상

에이전트 셸은 Coast를 중심으로 **퍼스트파티 통합을 구축하는 제품**을 위해 설계되었습니다 — 오케스트레이션 플랫폼, 에이전트 래퍼, 그리고 `input`, `read-output`, `session-status` API를 통해 컨테이너화된 코딩 에이전트를 프로그래밍 방식으로 관리하려는 도구들입니다.

일반적인 병렬 에이전트 코딩에는 호스트에서 에이전트를 실행하세요. 더 단순하고, OAuth 문제를 피하며, 자격 증명 마운트 복잡성을 우회하고, 공유 파일시스템을 최대한 활용할 수 있습니다. 에이전트 컨테이너화 오버헤드 없이 Coast의 모든 이점(격리된 런타임, 포트 관리, 워크트리 전환)을 얻습니다.

에이전트 셸보다 한 단계 더 복잡한 수준은 컨테이너화된 에이전트가 도구에 접근할 수 있도록 [MCP 서버](MCP_SERVERS.md)를 Coast에 마운트하는 것입니다. 이는 통합 표면을 더 확장하며 별도로 다룹니다. 필요하다면 해당 기능을 사용할 수 있지만, 대부분의 사용자는 사용하지 않는 것이 좋습니다.

# Coasts 시작하기

아직 설치와 요구 사항을 완료하지 않았다면, 아래 내용을 먼저 완료하세요. 그런 다음 이 가이드는 프로젝트에서 Coast를 사용하는 방법을 안내합니다.

## Installing

- `curl -fsSL https://coasts.dev/install | sh`
- `coast daemon install`

*`coast daemon install`을 실행하지 않기로 결정했다면, 매번 `coast daemon start`로 데몬을 수동으로 시작할 책임은 사용자에게 있습니다.*

## Requirements

- macOS
- Docker Desktop
- Git을 사용하는 프로젝트
- Node.js
- `socat` *(Homebrew `depends_on` 의존성으로 `curl -fsSL https://coasts.dev/install | sh`와 함께 설치됨)*

```text
Linux note: We have not tested Coasts on Linux yet, but Linux support is planned.
You can try to run Coasts on Linux today, but we do not provide guarantees that it will work correctly.
```

## Setting Up Coasts in a Project

프로젝트 루트에 Coastfile을 추가하세요. 설치할 때 worktree에 있지 않은지 확인하세요.

```text
my-project/
├── Coastfile              <-- this is what Coast reads
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

`Coastfile`은 기존 로컬 개발 리소스를 가리키고 Coasts 전용 구성을 추가합니다 — 전체 스키마는 [Coastfiles 문서](coastfiles/README.md)를 참고하세요:

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Coastfile은 *일반적으로* 기존 `docker-compose.yml`을 가리키는(컨테이너를 쓰지 않는 로컬 개발 구성에서도 동작합니다) 가벼운 TOML 파일이며, 프로젝트를 병렬로 실행하기 위해 필요한 변경 사항(포트 매핑, 볼륨 전략, 시크릿)을 설명합니다. 프로젝트 루트에 두세요.

프로젝트용 Coastfile을 만드는 가장 빠른 방법은 코딩 에이전트에게 맡기는 것입니다.

Coasts CLI에는 어떤 AI 에이전트에게든 Coastfile 전체 스키마와 CLI를 가르쳐주는 내장 프롬프트가 포함되어 있습니다. 여기에서 볼 수 있습니다: [installation_prompt.txt](installation_prompt.txt)

에이전트에 직접 전달하거나, [installation prompt](installation_prompt.txt)를 복사해 에이전트의 채팅에 붙여넣으세요:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

이 프롬프트는 Coastfile TOML 형식, 볼륨 전략, 시크릿 주입, 그리고 관련된 모든 CLI 명령을 다룹니다. 에이전트가 프로젝트를 분석하고 Coastfile을 생성합니다.

## Your First Coast

첫 번째 Coast를 시작하기 전에, 실행 중인 개발 환경을 모두 내려주세요. Docker Compose를 사용 중이라면 `docker-compose down`을 실행하세요. 로컬 개발 서버가 실행 중이라면 중지하세요. Coasts는 자체 포트를 관리하며 이미 리스닝 중인 무엇과도 충돌합니다.

Coastfile이 준비되면:

```bash
coast build
coast run dev-1
```

인스턴스가 실행 중인지 확인하세요:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

서비스가 어떤 포트에서 리스닝 중인지 확인하세요:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

각 인스턴스는 고유한 동적 포트 세트를 가지므로 여러 인스턴스를 나란히 실행할 수 있습니다. 인스턴스를 프로젝트의 표준(캐노니컬) 포트로 다시 매핑하려면 체크아웃하세요:

```bash
coast checkout dev-1
```

이는 이제 런타임이 체크아웃되었고, 프로젝트의 표준 포트(예: `3000`, `5432`)가 이 Coast 인스턴스로 라우팅된다는 뜻입니다.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

프로젝트용 Coastguard 관측(Observability) UI를 띄우려면:

```bash
coast ui
```

## What's Next?

- 호스트 에이전트가 Coasts와 상호작용하는 방법을 알 수 있도록 [호스트 에이전트용 스킬](SKILLS_FOR_HOST_AGENTS.md)을 설정하세요

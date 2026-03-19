# Coasts 시작하기

```youtube
Je921fgJ4RY
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## 설치

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*`coast daemon install`을 실행하지 않기로 결정했다면, 매번 `coast daemon start`로 데몬을 수동으로 시작할 책임이 있습니다.*

## 요구 사항

- macOS 또는 Linux
- macOS에서는 Docker Desktop, Linux에서는 Compose 플러그인이 포함된 Docker Engine
- Git을 사용하는 프로젝트
- Node.js
- `socat` (macOS에서는 `brew install socat`, Ubuntu에서는 `sudo apt install socat`)

```text
Linux 참고: 동적 포트는 Linux에서 별도 설정 없이 바로 동작합니다.
`1024` 미만의 표준 포트가 필요하다면, 필요한 호스트 설정은 checkout 문서를 참고하세요.
```

## 프로젝트에서 Coasts 설정하기

프로젝트 루트에 Coastfile을 추가하세요. 설치할 때 worktree 위에 있지 않은지 확인하세요.

```text
my-project/
├── Coastfile              <-- Coast가 읽는 파일
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

Coastfile은 *일반적으로* 기존의 `docker-compose.yml`을 가리키는 가벼운 TOML 파일입니다(컨테이너화되지 않은 로컬 개발 설정에서도 동작합니다). 또한 프로젝트를 병렬로 실행하기 위해 필요한 수정 사항(포트 매핑, 볼륨 전략, 시크릿)을 설명합니다. 프로젝트 루트에 배치하세요.

프로젝트용 Coastfile을 만드는 가장 빠른 방법은 코딩 에이전트가 만들도록 하는 것입니다.

Coasts CLI에는 어떤 AI 에이전트에게든 Coastfile 전체 스키마와 CLI를 가르쳐주는 내장 프롬프트가 포함되어 있습니다. 이를 에이전트의 채팅에 복사해 넣으면 프로젝트를 분석하고 Coastfile을 생성합니다.

```prompt-copy
installation_prompt.txt
```

또한 `coast installation-prompt`를 실행하면 CLI에서 동일한 출력을 얻을 수 있습니다.

## 첫 번째 Coast

첫 번째 Coast를 시작하기 전에, 실행 중인 모든 개발 환경을 내려주세요. Docker Compose를 사용 중이라면 `docker-compose down`을 실행하세요. 로컬 개발 서버가 실행 중이라면 중지하세요. Coasts는 자체 포트를 관리하며 이미 리스닝 중인 항목과 충돌합니다.

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

서비스가 어디에서 리스닝 중인지 확인하세요:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

각 인스턴스는 여러 인스턴스가 나란히 실행될 수 있도록 고유한 동적 포트 세트를 받습니다. 인스턴스를 프로젝트의 표준 포트에 매핑하려면 체크아웃하세요:

```bash
coast checkout dev-1
```

이는 런타임이 이제 체크아웃되었음을 의미하며, 프로젝트의 표준 포트(예: `3000`, `5432`)가 이 Coast 인스턴스로 라우팅됩니다.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

프로젝트의 Coastguard 관측 UI를 띄우려면:

```bash
coast ui
```

## 다음은?

- Coasts와 상호작용하는 방법을 알 수 있도록 [호스트 에이전트를 위한 skill](SKILLS_FOR_HOST_AGENTS.md)을 설정하세요

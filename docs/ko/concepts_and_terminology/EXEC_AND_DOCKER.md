# Exec & Docker

`coast exec`는 Coast의 DinD 컨테이너 안의 셸로 들어가게 해줍니다. 작업 디렉터리는 `/workspace` — Coastfile이 있는 [바인드 마운트된 프로젝트 루트](FILESYSTEM.md)입니다. 이는 호스트 머신에서 Coast 내부의 명령을 실행하고, 파일을 살펴보거나, 서비스를 디버깅하는 기본 방법입니다.

`coast docker`는 내부 Docker 데몬과 직접 통신하기 위한 동반 명령입니다.

## `coast exec`

Coast 인스턴스 내부에서 셸을 엽니다:

```bash
coast exec dev-1
```

이 명령은 `/workspace`에서 `sh` 세션을 시작합니다. Coast 컨테이너는 Alpine 기반이므로 기본 셸은 `bash`가 아니라 `sh`입니다.

대화형 셸에 들어가지 않고 특정 명령만 실행할 수도 있습니다:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
```

인스턴스 이름 뒤의 모든 내용은 명령으로 전달됩니다. `--`를 사용해, 실행하려는 명령에 속한 플래그와 `coast exec`에 속한 플래그를 구분하세요.

### Working Directory

셸은 `/workspace`에서 시작하며, 이는 호스트의 프로젝트 루트가 컨테이너에 바인드 마운트된 것입니다. 즉, 소스 코드, Coastfile, 그리고 모든 프로젝트 파일이 바로 그곳에 있습니다:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

`/workspace` 아래의 파일을 변경하면 호스트에 즉시 반영됩니다 — 복사본이 아니라 바인드 마운트입니다.

### Interactive vs Non-Interactive

stdin이 TTY(터미널에서 직접 타이핑)일 때, `coast exec`는 데몬을 완전히 우회하고 전체 TTY 패스스루를 위해 `docker exec -it`를 직접 실행합니다. 이는 색상, 커서 이동, 탭 자동완성, 대화형 프로그램이 모두 기대한 대로 동작한다는 뜻입니다.

stdin이 파이프되거나 스크립트로 실행될 때(CI, 에이전트 워크플로, `coast exec dev-1 -- some-command | grep foo`), 요청은 데몬을 통해 처리되며 구조화된 stdout, stderr, 그리고 종료 코드를 반환합니다.

### File Permissions

exec는 호스트 사용자와 동일한 UID:GID로 실행되므로, Coast 내부에서 생성된 파일은 호스트에서 올바른 소유권을 갖습니다. 호스트와 컨테이너 사이에 권한 불일치가 없습니다.

## `coast docker`

`coast exec`가 DinD 컨테이너 자체 안에서 셸을 제공하는 반면, `coast docker`는 **내부** Docker 데몬 — compose 서비스를 관리하는 데몬 — 을 대상으로 Docker CLI 명령을 실행할 수 있게 해줍니다.

```bash
coast docker dev-1                    # defaults to: docker ps
coast docker dev-1 ps                 # same as above
coast docker dev-1 compose ps         # docker compose ps (inner services)
coast docker dev-1 images             # list images in the inner daemon
coast docker dev-1 compose logs web   # docker compose logs for a service
```

전달하는 모든 명령은 자동으로 `docker`가 접두사로 붙습니다. 따라서 `coast docker dev-1 compose ps`는 Coast 컨테이너 내부에서 `docker compose ps`를 실행하며, 내부 데몬과 통신합니다.

### `coast exec` vs `coast docker`

차이는 무엇을 대상으로 하느냐입니다:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | `sh -c "ls /workspace"` in DinD container | Coast 컨테이너 자체 (프로젝트 파일, 설치된 도구) |
| `coast docker dev-1 ps` | `docker ps` in DinD container | 내부 Docker 데몬 (compose 서비스 컨테이너) |
| `coast docker dev-1 compose logs web` | `docker compose logs web` in DinD container | 내부 데몬을 통해 특정 compose 서비스의 로그 |

프로젝트 수준 작업 — 테스트 실행, 의존성 설치, 파일 검사 — 에는 `coast exec`를 사용하세요. 내부 Docker 데몬이 무엇을 하고 있는지 — 컨테이너 상태, 이미지, 네트워크, compose 작업 — 확인해야 할 때는 `coast docker`를 사용하세요.

## Coastguard Exec Tab

Coastguard 웹 UI는 WebSocket으로 연결된 영구적인 대화형 터미널을 제공합니다.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Coast 인스턴스 내부의 /workspace에서 셸 세션을 보여주는 Coastguard Exec 탭.*

터미널은 xterm.js로 구동되며 다음을 제공합니다:

- **영구 세션** — 터미널 세션은 페이지 이동과 브라우저 새로고침 이후에도 유지됩니다. 재연결 시 스크롤백 버퍼를 재생하여 중단한 지점부터 이어갈 수 있습니다.
- **여러 탭** — 여러 셸을 동시에 열 수 있습니다. 각 탭은 독립적인 세션입니다.
- **[에이전트 셸](AGENT_SHELLS.md) 탭** — AI 코딩 에이전트를 위한 전용 에이전트 셸을 생성하며, 활성/비활성 상태 추적을 지원합니다.
- **전체 화면 모드** — 터미널을 화면 전체로 확장합니다(나가려면 Escape).

인스턴스 수준의 exec 탭을 넘어, Coastguard는 다른 수준에서도 터미널 접근을 제공합니다:

- **서비스 exec** — Services 탭에서 개별 서비스를 클릭하여 해당 특정 내부 컨테이너에서 셸을 엽니다(이는 `docker exec`를 두 번 수행합니다 — 먼저 DinD 컨테이너로, 그다음 서비스 컨테이너로).
- **[공유 서비스](SHARED_SERVICES.md) exec** — 호스트 수준의 공유 서비스 컨테이너 내부에서 셸을 엽니다.
- **호스트 터미널** — Coast에 들어가지 않고도, 프로젝트 루트에서 호스트 머신의 셸을 제공합니다.

## When to Use Which

- **`coast exec`** — DinD 컨테이너 내부에서 프로젝트 수준 명령(npm install, go test, 파일 검사, 디버깅)을 실행합니다.
- **`coast docker`** — 내부 Docker 데몬을 점검하거나 관리합니다(컨테이너 상태, 이미지, 네트워크, compose 작업).
- **Coastguard Exec tab** — 영구 세션, 여러 탭, 에이전트 셸 지원을 통한 대화형 디버깅. UI의 나머지 부분을 탐색하면서 여러 터미널을 열어두고 싶을 때 가장 적합합니다.
- **`coast logs`** — 서비스 출력을 읽을 때는 `coast docker compose logs` 대신 `coast logs`를 사용하세요. [Logs](LOGS.md)를 참고하세요.
- **`coast ps`** — 서비스 상태를 확인할 때는 `coast docker compose ps` 대신 `coast ps`를 사용하세요. [Runtimes and Services](RUNTIMES_AND_SERVICES.md)를 참고하세요.

# Exec & Docker

`coast exec`는 Coast의 DinD 컨테이너 내부의 셸로 들어가게 해줍니다. 작업 디렉터리는 `/workspace`이며, 이는 Coastfile이 위치한 [바인드 마운트된 프로젝트 루트](FILESYSTEM.md)입니다. 이것은 호스트 머신에서 Coast 내부의 명령을 실행하고, 파일을 확인하거나, 서비스를 디버그하는 기본 방법입니다.

`coast docker`는 내부 Docker 데몬과 직접 통신하기 위한 짝이 되는 명령입니다.

## `coast exec`

Coast 인스턴스 내부에서 셸 열기:

```bash
coast exec dev-1
```

이 명령은 `/workspace`에서 `sh` 세션을 시작합니다. Coast 컨테이너는 Alpine 기반이므로 기본 셸은 `bash`가 아니라 `sh`입니다.

대화형 셸에 들어가지 않고 특정 명령만 실행할 수도 있습니다:

```bash
coast exec dev-1 ls -la
coast exec dev-1 -- npm install
coast exec dev-1 -- go test ./...
coast exec dev-1 --service web
coast exec dev-1 --service web -- php artisan test
```

인스턴스 이름 뒤의 모든 내용은 명령으로 전달됩니다. `coast exec`에 속한 플래그와 여러분의 명령에 속한 플래그를 구분하려면 `--`를 사용하세요.

외부 Coast 컨테이너 대신 특정 compose 서비스 컨테이너를 대상으로 하려면 `--service <name>`을 전달하세요. Coast의 기본 호스트 UID:GID 매핑 대신 순수한 컨테이너 루트 접근이 필요할 때는 `--root`를 전달하세요.

### Working Directory

셸은 `/workspace`에서 시작하며, 이는 호스트 프로젝트 루트가 컨테이너에 바인드 마운트된 위치입니다. 즉, 소스 코드, Coastfile, 그리고 모든 프로젝트 파일이 바로 그곳에 있습니다:

```text
/workspace $ ls
Coastfile       README.md       apps/           packages/
Coastfile.light go.work         infra/          scripts/
Coastfile.snap  go.work.sum     package-lock.json
```

`/workspace` 아래의 파일에 변경을 가하면 그 내용은 즉시 호스트에 반영됩니다 — 이것은 복사본이 아니라 바인드 마운트입니다.

### Interactive vs Non-Interactive

stdin이 TTY인 경우(터미널에 직접 입력 중일 때), `coast exec`는 데몬을 완전히 우회하고 전체 TTY 패스스루를 위해 직접 `docker exec -it`를 실행합니다. 즉, 색상, 커서 이동, 탭 완성, 대화형 프로그램이 모두 예상대로 동작합니다.

stdin이 파이프로 전달되거나 스크립트에서 실행되는 경우(CI, 에이전트 워크플로, `coast exec dev-1 -- some-command | grep foo`), 요청은 데몬을 통해 전달되며 구조화된 stdout, stderr, 그리고 종료 코드를 반환합니다.

### File Permissions

exec는 호스트 사용자 UID:GID로 실행되므로 Coast 내부에서 생성된 파일은 호스트에서도 올바른 소유권을 가집니다. 호스트와 컨테이너 사이에 권한 불일치가 발생하지 않습니다.

## `coast docker`

`coast exec`가 DinD 컨테이너 자체 안의 셸을 제공하는 반면, `coast docker`는 **내부** Docker 데몬 — 즉 compose 서비스를 관리하는 데몬 — 을 대상으로 Docker CLI 명령을 실행할 수 있게 해줍니다.

```bash
coast docker dev-1                    # 기본값: docker ps
coast docker dev-1 ps                 # 위와 동일
coast docker dev-1 compose ps         # 현재 Coast가 관리하는 활성 스택에 대한 docker compose ps
coast docker dev-1 images             # 내부 데몬의 이미지 목록
coast docker dev-1 compose logs web   # 서비스에 대한 docker compose logs
```

여러분이 전달하는 모든 명령 앞에는 자동으로 `docker`가 붙습니다. 따라서 `coast docker dev-1 compose ps`는 내부 데몬과 통신하면서 Coast 컨테이너 안에서 `docker compose ps`를 실행합니다.

### `coast exec` vs `coast docker`

차이는 무엇을 대상으로 하느냐입니다:

| Command | Runs as | Target |
|---|---|---|
| `coast exec dev-1 ls /workspace` | DinD 컨테이너에서 `sh -c "ls /workspace"` | Coast 컨테이너 자체 (프로젝트 파일, 설치된 도구) |
| `coast exec dev-1 --service web` | 확인된 내부 서비스 컨테이너에서 `docker exec ... sh` | 특정 compose 서비스 컨테이너 |
| `coast docker dev-1 ps` | DinD 컨테이너에서 `docker ps` | 내부 Docker 데몬 (compose 서비스 컨테이너들) |
| `coast docker dev-1 compose logs web` | DinD 컨테이너에서 `docker compose logs web` | 내부 데몬을 통한 특정 compose 서비스의 로그 |

프로젝트 수준 작업 — 테스트 실행, 의존성 설치, 파일 확인 — 에는 `coast exec`를 사용하세요. 내부 Docker 데몬이 무엇을 하고 있는지 — 컨테이너 상태, 이미지, 네트워크, compose 작업 — 확인해야 할 때는 `coast docker`를 사용하세요.

## Coastguard Exec Tab

Coastguard 웹 UI는 WebSocket으로 연결되는 지속형 대화형 터미널을 제공합니다.

![Exec tab in Coastguard](../../assets/coastguard-exec.png)
*Coast 인스턴스 내부 /workspace에서 셸 세션을 보여주는 Coastguard Exec 탭.*

이 터미널은 xterm.js로 구동되며 다음을 제공합니다:

- **지속형 세션** — 터미널 세션은 페이지 이동과 브라우저 새로고침 후에도 유지됩니다. 다시 연결하면 스크롤백 버퍼가 재생되어 중단한 지점부터 이어갈 수 있습니다.
- **여러 탭** — 여러 셸을 동시에 열 수 있습니다. 각 탭은 독립적인 세션입니다.
- **[Agent shell](AGENT_SHELLS.md) tabs** — AI 코딩 에이전트를 위한 전용 에이전트 셸을 생성하며, 활성/비활성 상태 추적을 지원합니다.
- **전체 화면 모드** — 터미널을 화면 전체로 확장합니다(종료하려면 Escape).

인스턴스 수준의 exec 탭 외에도 Coastguard는 다른 수준의 터미널 접근도 제공합니다:

- **Service exec** — Services 탭에서 개별 서비스를 클릭하면 해당 특정 내부 컨테이너 안의 셸로 들어갈 수 있습니다(이 경우 `docker exec`를 두 번 수행합니다 — 먼저 DinD 컨테이너로, 그다음 서비스 컨테이너로).
- **[Shared service](SHARED_SERVICES.md) exec** — 호스트 수준 공유 서비스 컨테이너 내부의 셸로 들어갑니다.
- **Host terminal** — Coast에 전혀 들어가지 않고 프로젝트 루트에서 호스트 머신의 셸을 엽니다.

## When to Use Which

- **`coast exec`** — DinD 컨테이너 내부에서 프로젝트 수준 명령을 실행하거나, `--service`를 전달해 특정 compose 서비스 컨테이너 안에서 셸을 열거나 명령을 실행합니다.
- **`coast docker`** — 내부 Docker 데몬을 점검하거나 관리합니다(컨테이너 상태, 이미지, 네트워크, compose 작업).
- **Coastguard Exec tab** — 지속형 세션, 여러 탭, 에이전트 셸 지원이 있는 대화형 디버깅용입니다. UI의 나머지 부분을 탐색하면서 여러 터미널을 열어두고 싶을 때 가장 적합합니다.
- **`coast logs`** — 서비스 출력을 읽을 때는 `coast docker compose logs` 대신 `coast logs`를 사용하세요. [Logs](LOGS.md)를 참조하세요.
- **`coast ps`** — 서비스 상태를 확인할 때는 `coast docker compose ps` 대신 `coast ps`를 사용하세요. [Runtimes and Services](RUNTIMES_AND_SERVICES.md)를 참조하세요.

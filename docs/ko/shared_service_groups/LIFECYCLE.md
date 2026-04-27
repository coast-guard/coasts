# SSG 라이프사이클

각 프로젝트의 SSG는 `<project>-ssg`라는 이름의 자체적인 외부 Docker-in-Docker 컨테이너입니다(예: `cg-ssg`). 라이프사이클 동사는 cwd의 `Coastfile`을 소유한 프로젝트(또는 `--working-dir`로 지정한 프로젝트)의 SSG를 대상으로 합니다. 모든 변경 명령은 데몬의 프로젝트별 mutex를 통해 직렬화되므로, 동일한 프로젝트에 대해 동시에 실행된 두 개의 `coast ssg run` / `coast ssg stop` 호출은 경쟁(race)하지 않고 큐에 쌓입니다. 반면 서로 다른 두 프로젝트는 각자의 SSG를 병렬로 변경할 수 있습니다.

## 상태 머신

```text
                     coast ssg build           coast ssg run
(no build)   -->  built     -->     created    -->     running
                                                          |
                                                   coast ssg stop
                                                          v
                                                       stopped
                                                          |
                                                  coast ssg start
                                                          v
                                                       running
                                                          |
                                                   coast ssg rm
                                                          v
                                                      (removed)
```

- `coast ssg build`는 컨테이너를 생성하지 않습니다. 이 명령은 `~/.coast/ssg/<project>/builds/<id>/` 아래 디스크에 아티팩트를 생성하고, (`[secrets.*]`가 선언된 경우) secret 값을 keystore로 추출합니다.
- `coast ssg run`은 `<project>-ssg` DinD를 생성하고, 동적 호스트 포트를 할당하며, 선언된 secret을 실행별 `compose.override.yml`에 구체화하고, 내부 compose 스택을 부팅합니다.
- `coast ssg stop`은 외부 DinD를 중지하지만, 컨테이너와 동적 포트 행, 프로젝트별 가상 포트는 보존하므로 `start`가 빠릅니다.
- `coast ssg start`는 SSG를 다시 시작하고 secret을 다시 구체화합니다(따라서 stop과 start 사이에 `coast ssg secrets clear`를 실행하면 그 변경이 반영됩니다).
- `coast ssg rm`은 외부 DinD 컨테이너를 제거합니다. `--with-data`를 사용하면 내부 named volume도 삭제합니다(호스트 bind-mount 내용은 절대 건드리지 않습니다). keystore는 `rm`으로는 절대 지워지지 않으며, 오직 `coast ssg secrets clear`만이 이를 지웁니다.
- `coast ssg restart`는 `stop` + `start`를 편의상 감싼 래퍼입니다.

## 명령어

### `coast ssg run`

`<project>-ssg` DinD가 존재하지 않으면 생성하고 내부 서비스를 시작합니다. 선언된 서비스마다 하나의 동적 호스트 포트를 할당하고 이를 외부 DinD에 게시합니다. 이 매핑은 상태 DB에 기록되므로 포트 할당기가 이를 재사용하지 않습니다.

```bash
coast ssg run
```

진행 이벤트는 `coast ssg build`와 동일한 `BuildProgressEvent` 채널을 통해 스트리밍됩니다. 기본 플랜은 7단계입니다:

1. SSG 준비
2. SSG 컨테이너 생성
3. SSG 컨테이너 시작
4. 내부 데몬 대기
5. 캐시된 이미지 로드
6. secret 구체화 (`[secrets]` 블록이 없으면 조용히 넘어가며, 있으면 secret별 항목을 출력)
7. 내부 서비스 시작

**자동 시작**. SSG 서비스를 참조하는 consumer Coast에서 `coast run`을 실행하면, SSG가 아직 실행 중이 아닐 경우 자동으로 시작됩니다. 언제든 명시적으로 `coast ssg run`을 실행할 수 있지만, 실제로 그럴 필요는 드뭅니다. 자세한 내용은 [Consuming -> Auto-start](CONSUMING.md#auto-start)를 참조하세요.

### `coast ssg start`

이전에 중지된 SSG를 시작합니다. 기존 `<project>-ssg` 컨테이너가 필요합니다(즉, 이전에 `coast ssg run`이 수행되어 있어야 함). keystore에서 secret을 다시 구체화하여 stop 이후의 변경 사항을 반영한 다음, stop 이전에 체크아웃되어 있던 canonical port에 대해 호스트 측 checkout socat을 다시 띄웁니다.

```bash
coast ssg start
```

### `coast ssg stop`

외부 DinD 컨테이너를 중지합니다. 내부 compose 스택도 함께 내려갑니다. 컨테이너, 동적 포트 할당, 프로젝트별 가상 포트 행은 보존되므로 다음 `start`는 빠릅니다.

```bash
coast ssg stop
coast ssg stop --force
```

호스트 측 checkout socat은 종료되지만, 상태 DB의 해당 행은 유지됩니다. 다음 `coast ssg start` 또는 `coast ssg run`이 이를 다시 띄웁니다. 자세한 내용은 [Checkout](CHECKOUT.md)를 참조하세요.

**원격 consumer 게이트.** 어떤 원격 shadow Coast(`coast assign --remote ...`로 생성된 것)가 현재 이를 소비 중이면 데몬은 SSG 중지를 거부합니다. reverse SSH 터널을 강제로 내리고 계속 진행하려면 `--force`를 전달하세요. 자세한 내용은 [Consuming -> Remote Coasts](CONSUMING.md#remote-coasts)를 참조하세요.

### `coast ssg restart`

`stop` + `start`와 동일합니다. 컨테이너와 동적 포트 매핑은 보존됩니다.

```bash
coast ssg restart
```

### `coast ssg rm`

외부 DinD 컨테이너를 제거합니다. 기본적으로 내부 named volume(Postgres WAL 등)은 보존되므로, `rm` / `run` 주기 사이에도 데이터가 유지됩니다. 호스트 bind-mount 내용은 절대 건드리지 않습니다.

```bash
coast ssg rm                    # named volume 보존; keystore 보존
coast ssg rm --with-data        # named volume도 삭제; 그래도 keystore는 보존
coast ssg rm --force            # 원격 consumer가 있어도 진행
```

- `--with-data`는 DinD 자체를 제거하기 전에 모든 내부 named volume을 삭제합니다. 새 데이터베이스로 시작하고 싶을 때 사용하세요.
- `--force`는 원격 shadow Coast가 SSG를 참조하고 있어도 계속 진행합니다. 의미는 `stop --force`와 동일합니다.
- `rm`은 `ssg_port_checkouts` 행을 삭제합니다(canonical-port 호스트 바인딩에 대해서는 파괴적임).

SSG 고유 secret이 저장되는 keystore(`coast_image = "ssg:<project>"`)는 `rm`이나 `rm --with-data`의 영향을 **받지 않습니다**. SSG secret을 지우려면 `coast ssg secrets clear`를 사용하세요([Secrets](SECRETS.md) 참조).

### `coast ssg ps`

현재 프로젝트 SSG의 서비스 상태를 표시합니다. 빌드된 구성을 위해 `manifest.json`을 읽고, 실행 중인 컨테이너 메타데이터를 위해 라이브 상태 DB를 검사합니다.

```bash
coast ssg ps
```

성공적으로 `run`한 뒤의 출력:

```text
SSG build: b455787d95cfdeb_20260420061903  (project: cg, running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

서비스별 canonical / dynamic / virtual 포트 매핑을 표시하며, 해당 서비스에 대해 호스트 측 canonical-port socat이 살아 있으면 `(checked out)` 주석을 붙입니다. virtual 포트는 consumer가 실제로 연결하는 포트입니다. 자세한 내용은 [Routing](ROUTING.md)을 참조하세요.

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
#   postgres             5432            54201           42000      (checked out)
#   redis                6379            54202           42001
```

### `coast ssg logs`

외부 DinD 컨테이너 또는 특정 내부 서비스의 로그를 스트리밍합니다.

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>`은 compose 키로 내부 서비스를 지정합니다. 이를 생략하면 외부 DinD의 stdout을 받습니다.
- `--tail N`은 과거 로그 줄 수를 제한합니다(기본값 200).
- `--follow` / `-f`는 `Ctrl+C`를 누를 때까지 새 줄이 도착하는 대로 스트리밍합니다.

### `coast ssg exec`

외부 DinD 또는 내부 서비스 안에서 명령을 실행합니다.

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- `--service` 없이 사용하면 명령은 외부 `<project>-ssg` 컨테이너에서 실행됩니다.
- `--service <name>`과 함께 사용하면 명령은 해당 compose 서비스 내부에서 `docker compose exec -T`를 통해 실행됩니다.
- `--` 뒤의 모든 내용은 플래그를 포함해 그대로 하위 `docker exec`로 전달됩니다.

### `coast ssg ls`

데몬이 알고 있는 모든 프로젝트의 SSG를 나열합니다. 이것은 cwd에서 프로젝트를 해석하지 않는 유일한 동사이며, 데몬의 SSG 상태에 있는 모든 항목의 행을 반환합니다.

```bash
coast ssg ls

#   PROJECT     STATUS     BUILD                                       SERVICES   CREATED
#   cg          running    b455787d95cfdeb_20260420061903               2          2026-04-20T06:19:03Z
#   filemap     stopped    b9b93fdb41b21337_20260418123012               3          2026-04-18T12:30:12Z
```

이전 프로젝트에서 잊고 남겨둔 SSG를 찾거나, 이 머신에서 어떤 프로젝트가 어떤 상태로든 SSG를 가지고 있는지 빠르게 확인할 때 유용합니다.

## Mutex 의미론

모든 변경 SSG 동사(`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`)는 실제 핸들러로 디스패치되기 전에 데몬 내부에서 프로젝트별 SSG mutex를 획득합니다. 동일한 프로젝트에 대한 두 개의 동시 호출은 큐에 쌓이고, 서로 다른 프로젝트에 대해서는 병렬로 실행됩니다. 읽기 전용 동사(`ps`/`ports`/`logs`/`exec`/`doctor`/`ls`)는 mutex를 획득하지 않습니다.

## Coastguard 통합

[Coastguard](../concepts_and_terminology/COASTGUARD.md)를 실행 중이라면, SPA는 SSG 라이프사이클을 자체 페이지(`/project/<p>/ssg/local`)에 렌더링하며, Exec, Ports, Services, Logs, Secrets, Stats, Images, Volumes 탭을 제공합니다. consumer Coast가 자동 시작을 트리거할 때마다 `CoastEvent::SsgStarting` 및 `CoastEvent::SsgStarted`가 발생하므로, UI는 어떤 프로젝트가 부팅을 필요로 했는지 이를 귀속시킬 수 있습니다.

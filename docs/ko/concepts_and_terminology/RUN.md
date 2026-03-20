# 실행

`coast run`은 새 Coast 인스턴스를 생성합니다. 최신 [build](BUILDS.md)를 확인하고, [DinD 컨테이너](RUNTIMES_AND_SERVICES.md)를 프로비저닝하며, 캐시된 이미지를 로드하고, compose 서비스를 시작하고, [동적 포트](PORTS.md)를 할당하고, 상태 데이터베이스에 인스턴스를 기록합니다.

```bash
coast run dev-1
```

`-w`를 전달하면, Coast는 프로비저닝이 완료된 후 해당 워크트리도 [할당](ASSIGN.md)합니다:

```bash
coast run dev-1 -w feature/oauth
```

이는 하네스나 에이전트가 워크트리를 생성하고, 한 단계에서 그에 대한 Coast도 함께 필요로 할 때 가장 일반적인 패턴입니다.

## 무엇이 일어나는가

`coast run`은 네 단계로 실행됩니다:

1. **검증 및 삽입** — 이름이 고유한지 확인하고, build ID(`latest` 심볼릭 링크 또는 명시적인 `--build-id`에서)를 확인한 뒤, `Provisioning` 인스턴스 레코드를 삽입합니다.
2. **Docker 프로비저닝** — 호스트 데몬에서 DinD 컨테이너를 생성하고, 인스턴스별 이미지를 빌드하며, 캐시된 이미지 tarball을 내부 데몬에 로드하고, compose 파일을 다시 작성하고, 시크릿을 주입한 뒤, `docker compose up -d`를 실행합니다.
3. **마무리** — 포트 할당을 저장하고, 포트가 정확히 하나인 경우 기본 포트를 설정한 뒤, 인스턴스를 `Running` 상태로 전환합니다.
4. **선택적 워크트리 할당** — `-w <worktree>`가 제공된 경우, 새 인스턴스에 대해 `coast assign`을 실행합니다. 할당에 실패하더라도 Coast는 계속 실행되며, 실패는 경고로 기록됩니다.

DinD 컨테이너 내부의 영속적인 `/var/lib/docker` 볼륨 덕분에 이후 실행에서는 이미지 로드를 건너뜁니다. 차가운 캐시 상태의 새로운 `coast run`은 20초 이상 걸릴 수 있으며, `coast rm` 이후 재실행은 보통 10초 이내에 완료됩니다.

## CLI 사용법

```text
coast run <name> [options]
```

| Flag | Description |
|------|-------------|
| `-w`, `--worktree <name>` | 프로비저닝 완료 후 이 워크트리를 할당 |
| `--n <count>` | 배치 생성. 이름에 `{n}`이 포함되어야 함 (예: `coast run dev-{n} --n=5`는 dev-1부터 dev-5까지 생성) |
| `-t`, `--type <type>` | 타입이 지정된 빌드 사용 (예: `--type snap`은 `latest` 대신 `latest-snap`을 확인) |
| `--force-remove-dangling` | 생성 전에 같은 이름의 남아 있는 Docker 컨테이너 제거 |
| `-s`, `--silent` | 진행 상황 출력을 억제; 최종 요약 또는 오류만 출력 |
| `-v`, `--verbose` | Docker 빌드 로그를 포함한 자세한 세부 정보 표시 |

git 브랜치는 항상 현재 HEAD에서 자동 감지됩니다.

## 배치 생성

이름에 `{n}`과 `--n`을 사용하면 한 번에 여러 인스턴스를 생성할 수 있습니다:

```bash
coast run dev-{n} --n=5
```

이렇게 하면 `dev-1`, `dev-2`, `dev-3`, `dev-4`, `dev-5`가 순차적으로 생성됩니다. 각 인스턴스는 자체 DinD 컨테이너, 포트 할당, 볼륨 상태를 가집니다. 10개를 초과하는 배치는 확인을 요청합니다.

## 타입 지정 빌드

프로젝트에서 여러 Coastfile 타입을 사용하는 경우([Coastfile Types](COASTFILE_TYPES.md) 참조), `--type`을 전달하여 사용할 빌드를 선택합니다:

```bash
coast run dev-1                    # resolves "latest"
coast run test-1 --type test       # resolves "latest-test"
coast run snapshot-1 --type snap   # resolves "latest-snap"
```

## 실행 vs 할당 vs 제거

- `coast run`은 **새** 인스턴스를 생성합니다. 다른 Coast가 필요할 때 사용하세요.
- `coast assign`은 **기존** 인스턴스를 다른 워크트리로 다시 가리키게 합니다. 이미 Coast가 있고 어떤 코드를 실행할지 전환하고 싶을 때 사용하세요.
- `coast rm`은 인스턴스를 완전히 종료하고 제거합니다. Coast를 내리거나 처음부터 다시 만들고 싶을 때 사용하세요.

대부분의 일상적인 전환에는 `coast rm`이 필요하지 않습니다. 보통 `coast assign`과 `coast checkout`이면 충분합니다. 특히 Coastfile이나 build를 다시 빌드한 후 깔끔하게 다시 만들고 싶을 때 `coast rm`을 사용하세요.

이들을 조합할 수도 있습니다: `coast run dev-3 -w feature/billing`은 인스턴스를 생성하고 한 단계에서 워크트리를 할당합니다.

## 남아 있는 컨테이너

이전 `coast run`이 중단되었거나 `coast rm`이 완전히 정리되지 않았다면 "dangling Docker container" 오류가 나타날 수 있습니다. `--force-remove-dangling`을 전달하면 남아 있는 컨테이너를 제거하고 계속 진행합니다:

```bash
coast run dev-1 --force-remove-dangling
```

## 함께 보기

- [Remove](REMOVE.md) — 인스턴스를 완전히 종료하고 제거하기
- [Builds](BUILDS.md) — `coast run`이 사용하는 것
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — 각 인스턴스 내부의 DinD 아키텍처
- [Assign and Unassign](ASSIGN.md) — 기존 인스턴스를 다른 워크트리로 전환하기
- [Ports](PORTS.md) — 동적 포트와 정식 포트가 할당되는 방식
- [Coasts](COASTS.md) — Coast 인스턴스의 상위 개념

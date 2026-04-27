# 공유 서비스 그룹 사용하기

컨슈머 Coast는 컨슈머의 `Coastfile`에 있는 한 줄 플래그를 사용해 서비스별로 해당 프로젝트의 SSG 소유 서비스를 선택합니다. Coast 내부에서는 앱 컨테이너가 여전히 `postgres:5432`를 보며, 데몬의 라우팅 계층이 해당 트래픽을 안정적인 가상 포트를 통해 프로젝트의 `<project>-ssg` 외부 DinD로 리디렉션합니다.

`from_group = true`가 참조하는 SSG는 **항상 컨슈머 프로젝트 자신의 SSG**입니다. 프로젝트 간 공유는 없습니다. 컨슈머의 `[coast].name`이 `cg`이면, `from_group = true`는 `cg-ssg`의 `Coastfile.shared_service_groups`를 기준으로 해석됩니다.

## 구문

`from_group = true`가 포함된 `[shared_services.<name>]` 블록을 추가합니다:

```toml
# Consumer Coastfile
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true

# Optional per-project overrides:
inject = "env:DATABASE_URL"
# auto_create_db = true       # overrides the SSG service's default
```

TOML 키(이 예시에서는 `postgres`)는 프로젝트의 `Coastfile.shared_service_groups`에 선언된 서비스 이름과 일치해야 합니다.

## 금지된 필드

`from_group = true`를 사용할 때는 다음 필드가 파싱 시점에 거부됩니다:

- `image`
- `ports`
- `env`
- `volumes`

이들은 모두 SSG 쪽에 존재합니다. 이들 중 하나라도 `from_group = true`와 함께 나타나면, `coast build`는 다음과 같이 실패합니다:

```text
error: shared service 'postgres' has from_group = true; the following fields are forbidden: image, ports, env, volumes.
```

## 허용되는 오버라이드

컨슈머별로 다음 두 필드는 여전히 사용할 수 있습니다:

- `inject` -- 연결 문자열이 노출되는 환경 변수 또는 파일 경로입니다. 서로 다른 컨슈머 프로젝트는 같은 형태를 서로 다른 환경 변수 이름으로 노출할 수 있습니다.
- `auto_create_db` -- `coast run` 시점에 Coast가 이 서비스 내부에 인스턴스별 데이터베이스를 생성할지 여부입니다. SSG 서비스 자체의 `auto_create_db` 값을 덮어씁니다.

## 충돌 감지

하나의 Coastfile 안에서 이름이 같은 두 개의 `[shared_services.<name>]` 블록은 파싱 시점에 거부됩니다. 이 규칙은 그대로 유지됩니다.

프로젝트의 `Coastfile.shared_service_groups`에 선언되지 않은 이름을 참조하는 `from_group = true` 블록은 `coast build` 시점에 실패합니다:

```text
error: shared service 'postgres' has from_group = true but no service named 'postgres' is declared in Coastfile.shared_service_groups for project 'my-app'.
```

이것이 오타 검사입니다. 별도의 런타임 "드리프트" 검사는 없습니다 -- 컨슈머와 SSG 간의 형태 불일치는 빌드 시점 검사에서 드러나며, 런타임의 추가적인 불일치는 앱 관점에서 자연스럽게 연결 오류로 나타납니다.

## 자동 시작

컨슈머에서 `coast run`을 실행하면 프로젝트의 SSG가 아직 실행 중이 아닐 때 자동으로 시작됩니다:

- SSG 빌드는 존재하지만 컨테이너가 실행 중이 아님 -> 데몬이 프로젝트의 SSG mutex로 보호된 상태에서 `coast ssg start`에 해당하는 동작(또는 컨테이너가 한 번도 생성되지 않았다면 `run`)을 수행합니다.
- SSG 빌드가 전혀 존재하지 않음 -> 하드 에러:

  ```text
  Project 'my-app' references shared service 'postgres' from the Shared Service Group, but no SSG build exists. Run `coast ssg build` in the directory containing your Coastfile.shared_service_groups.
  ```

- SSG가 이미 실행 중임 -> 아무 작업도 하지 않으며, `coast run`이 즉시 계속됩니다.

진행 이벤트 `SsgStarting` 및 `SsgStarted`가 run 스트림에서 발생하므로 [Coastguard](../concepts_and_terminology/COASTGUARD.md)가 부팅을 컨슈머 프로젝트에 귀속시킬 수 있습니다.

## 라우팅 동작 방식

컨슈머 Coast 내부에서 앱 컨테이너는 세 가지 요소를 통해 `postgres:5432`를 프로젝트의 SSG로 해석합니다:

1. **별칭 IP + `extra_hosts`**가 컨슈머의 내부 compose에 `postgres -> <docker0 alias IP>`를 추가하여, `postgres`에 대한 DNS 조회가 성공하도록 합니다.
2. **In-DinD socat**가 `<alias>:5432`에서 수신하고 `host.docker.internal:<virtual_port>`로 전달합니다. 가상 포트는 `(project, service, container_port)`에 대해 안정적입니다 -- SSG를 다시 빌드해도 바뀌지 않습니다.
3. **Host socat**가 `<virtual_port>`에서 `127.0.0.1:<dynamic>`으로 전달하며, 여기서 `<dynamic>`은 SSG 컨테이너가 현재 publish한 포트입니다. Host socat는 SSG가 다시 빌드될 때 갱신되며, 컨슈머의 in-DinD socat는 전혀 변경할 필요가 없습니다.

앱 코드와 compose DNS는 바뀌지 않습니다. 프로젝트를 인라인 Postgres에서 SSG Postgres로 마이그레이션하는 것은 작은 Coastfile 수정(`image`/`ports`/`env` 제거, `from_group = true` 추가)과 재빌드만으로 가능합니다.

전체 홉별 동작 방식, 포트 개념, 그리고 설계 이유는 [Routing](ROUTING.md)을 참조하세요.

## `auto_create_db`

SSG Postgres 또는 MySQL 서비스에서 `auto_create_db = true`를 설정하면, 데몬은 실행되는 모든 컨슈머 Coast에 대해 해당 서비스 내부에 `{instance}_{project}` 데이터베이스를 생성합니다. 데이터베이스 이름은 인라인 `[shared_services]` 패턴이 생성하는 것과 일치하므로, `inject` URL은 `auto_create_db`가 생성하는 데이터베이스와 일치합니다.

생성은 멱등적입니다. 데이터베이스가 이미 존재하는 인스턴스에 대해 `coast run`을 다시 실행해도 아무 동작도 하지 않습니다. 기본 SQL은 인라인 경로와 동일하므로, 프로젝트가 어떤 패턴을 사용하든 DDL 출력은 바이트 단위까지 완전히 동일합니다.

컨슈머는 SSG 서비스의 `auto_create_db` 값을 덮어쓸 수 있습니다:

```toml
# SSG: auto_create_db = true, but this project doesn't want per-instance DBs.
[shared_services.postgres]
from_group = true
auto_create_db = false
```

## `inject`

`inject`는 앱 컨테이너에 연결 문자열을 노출합니다. 형식은 [Secrets](../coastfiles/SECRETS.md)와 동일합니다: `"env:NAME"`은 환경 변수를 만들고, `"file:/path"`는 컨슈머의 coast 컨테이너 내부에 파일을 쓴 뒤 이를 모든 스텁되지 않은 내부 compose 서비스에 읽기 전용으로 bind-mount합니다.

해석된 문자열은 동적 호스트 포트가 아니라 정식 서비스 이름과 정식 포트를 사용합니다. 이 불변성이 핵심입니다 -- 앱 컨테이너는 SSG가 어떤 동적 포트를 publish하고 있는지와 무관하게 항상 `postgres://coast:coast@postgres:5432/{db}`를 봅니다.

`env:NAME`과 `file:/path`는 모두 완전히 구현되어 있습니다.

이 `inject`는 **컨슈머 측** secret 파이프라인입니다: 값은 `coast build` 시점에 정식 SSG 메타데이터로부터 계산되어 컨슈머의 coast DinD에 주입됩니다. 이는 SSG의 *자체* 서비스가 소비할 값을 추출하는 **SSG 측** `[secrets.*]` 파이프라인([Secrets](SECRETS.md) 참조)과는 독립적입니다.

## 원격 Coast

원격 Coast(`coast assign --remote ...`로 생성된 것)는 reverse SSH 터널을 통해 로컬 SSG에 접근합니다. 로컬 데몬은 원격 머신에서 로컬 가상 포트로 되돌아가는 `ssh -N -R <vport>:localhost:<vport>`를 실행하며, 원격 DinD 내부에서 `extra_hosts: postgres: host-gateway`는 `postgres`를 원격의 host-gateway IP로 해석하고, SSH 터널은 반대편의 로컬 SSG를 동일한 가상 포트 번호에 연결합니다.

터널의 양쪽 모두 **가상** 포트를 사용하며, 동적 포트는 사용하지 않습니다. 이는 로컬에서 SSG를 다시 빌드해도 원격 터널이 무효화되지 않음을 의미합니다.

터널은 `(project, remote_host, service, container_port)` 단위로 병합됩니다 -- 같은 원격에 있는 동일 프로젝트의 여러 컨슈머 인스턴스는 하나의 `ssh -R` 프로세스를 공유합니다. 하나의 컨슈머를 제거해도 터널은 내려가지 않으며, 마지막 컨슈머가 제거될 때만 내려갑니다.

실질적인 결과:

- 원격 shadow Coast가 현재 SSG를 소비 중일 때는 `coast ssg stop` / `rm`이 거부됩니다. 데몬은 무엇이 SSG를 사용 중인지 알 수 있도록 차단 중인 shadow를 나열합니다.
- `coast ssg stop --force`(또는 `rm --force`)는 먼저 공유된 `ssh -R`을 내려놓은 뒤 계속 진행합니다. 원격 컨슈머가 연결을 잃게 됨을 감수할 때 사용하세요.

전체 원격 터널 아키텍처는 [Routing](ROUTING.md)을, 더 넓은 원격 머신 설정은 [Remote Coasts](../remote_coasts/README.md)를 참조하세요.

## 함께 보기

- [Routing](ROUTING.md) -- 정식 / 동적 / 가상 포트 개념과 전체 라우팅 체인
- [Secrets](SECRETS.md) -- 서비스 측 자격 증명을 위한 SSG 네이티브 `[secrets.*]` (컨슈머 측 `inject`와는 별개)
- [Coastfile: Shared Services](../coastfiles/SHARED_SERVICES.md) -- `from_group = true`를 포함한 전체 `[shared_services.*]` 스키마
- [Lifecycle](LIFECYCLE.md) -- 자동 시작을 포함해 `coast run`이 내부적으로 수행하는 작업
- [Checkout](CHECKOUT.md) -- 임시 도구를 위한 호스트 측 정식 포트 바인딩
- [Volumes](VOLUMES.md) -- 마운트와 권한; SSG를 다시 빌드할 때 새 Postgres 이미지가 데이터 디렉터리 소유권을 변경하는 경우 관련 있음

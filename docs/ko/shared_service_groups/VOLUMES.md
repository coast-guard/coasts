# SSG 볼륨

`[shared_services.<name>]` 내부에서 `volumes` 배열은 표준 Docker Compose 문법을 사용합니다:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

앞에 `/`가 붙으면 **호스트 바인드 경로**를 의미합니다 -- 바이트는 호스트 파일시스템에 저장되며 내부 서비스는 그 위치에서 직접 읽고 씁니다. 앞에 슬래시가 없으면, 예를 들어 `pg_wal:/var/lib/postgresql/wal` 같은 경우, 소스는 **SSG의 중첩 Docker 데몬 내부에 존재하는 Docker named volume**입니다 -- 이는 `coast ssg rm` 후에도 유지되며 `coast ssg rm --with-data`로 삭제됩니다. 두 형식 모두 허용됩니다.

파싱 단계에서 거부되는 항목: 상대 경로 (`./data:/...`), `..` 구성요소, 컨테이너 전용 볼륨(소스 없음), 그리고 하나의 서비스 내에서 중복된 대상 경로.

## docker-compose 또는 인라인 공유 서비스의 Docker 볼륨 재사용

이미 호스트 Docker named volume 안에 데이터가 있다면 -- `docker-compose up`으로 생성된 것, 인라인 `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]`로 생성된 것, 또는 직접 실행한 `docker volume create`로 만든 것 -- 볼륨의 실제 호스트 디렉터리를 bind-mount하여 SSG가 동일한 바이트를 읽도록 할 수 있습니다:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

왼쪽은 기존 Docker 볼륨의 호스트 파일시스템 경로입니다; `docker volume inspect <name>`에서 이를 `Mountpoint` 필드로 보여줍니다. Coast는 바이트를 복사하지 않습니다 -- SSG는 docker-compose가 사용하던 동일한 파일을 읽고 씁니다. `coast ssg rm` (`--with-data` 없이)은 이 볼륨을 건드리지 않으므로 docker-compose도 계속 사용할 수 있습니다.

> **그냥 `infra_postgres_data:/var/lib/postgresql/data`를 쓰면 안 되나요?** 인라인 `[shared_services.*]`에서는 동작합니다 (볼륨이 호스트 Docker 데몬에 생성되므로 docker-compose가 이를 볼 수 있습니다). 하지만 SSG 내부에서는 같은 방식으로 동작하지 *않습니다* -- 앞에 슬래시가 없는 이름은 SSG의 중첩 Docker 데몬 내부에 새 볼륨을 생성하며, 이는 호스트와 격리됩니다. 호스트 데몬에서 실행되는 다른 것들과 데이터를 공유하려면 대신 볼륨의 mountpoint 경로를 사용하세요.

### `coast ssg import-host-volume`

`coast ssg import-host-volume`는 `docker volume inspect`를 통해 볼륨의 `Mountpoint`를 확인하고, `/var/lib/docker/volumes/<name>/_data` 경로를 직접 구성하지 않아도 되도록 동등한 `volumes` 라인을 출력(또는 적용)합니다.

스니펫 모드(기본값)는 붙여넣을 TOML 조각을 출력합니다:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

출력은 새로운 `volumes = [...]` 항목이 이미 병합된 `[shared_services.postgres]` 블록입니다:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

적용 모드는 `Coastfile.shared_service_groups`를 제자리에서 다시 쓰고 원본은 `Coastfile.shared_service_groups.bak`에 저장합니다:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

플래그:

- `<VOLUME>` (위치 인자) -- 호스트 Docker named volume. 반드시 이미 존재해야 합니다 (`docker volume inspect`가 확인 방법입니다); 그렇지 않다면 먼저 `docker volume create`로 생성하거나 이름을 변경하세요.
- `--service` -- 수정할 `[shared_services.<name>]` 섹션. 이 섹션은 반드시 이미 존재해야 합니다.
- `--mount` -- 절대 컨테이너 경로. 상대 경로는 거부됩니다. 같은 서비스에서 중복된 마운트 경로는 즉시 오류가 됩니다.
- `--file` / `--working-dir` / `--config` -- SSG Coastfile 탐색 옵션으로, 규칙은 `coast ssg build`와 동일합니다.
- `--apply` -- Coastfile을 제자리에서 다시 씁니다. `--config`와 함께 사용할 수 없습니다 (인라인 텍스트는 다시 쓸 파일이 없기 때문입니다).

`.bak` 파일에는 원본 바이트가 그대로 들어 있으므로 적용 전 상태를 정확히 복구할 수 있습니다.

`/var/lib/docker/volumes/<name>/_data`는 Docker가 오랫동안 볼륨 mountpoint로 사용해 온 경로이며 오늘날 `docker volume inspect`도 이를 보고합니다. Docker가 이 경로를 영원히 유지하겠다고 공식적으로 보장하는 것은 아닙니다; 향후 Docker 릴리스에서 볼륨 위치가 바뀐다면 새 경로를 반영하기 위해 `coast ssg import-host-volume`를 다시 실행하세요.

## 권한

여러 이미지들은 데이터 디렉터리 소유자가 잘못되어 있으면 시작을 거부합니다. Postgres (debian 태그에서는 UID 999, alpine 태그에서는 UID 70), MySQL/MariaDB (UID 999), MongoDB (UID 999)가 흔한 사례입니다. 호스트 디렉터리 소유자가 root이면, Postgres는 시작 시 "data directory has wrong ownership"라는 짧은 메시지를 남기고 종료합니다.

해결 방법은 명령 하나입니다:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

이 작업은 `coast ssg run` 전에 수행하세요. 디렉터리가 아직 존재하지 않으면 `coast ssg run`이 기본 소유권으로 디렉터리를 생성합니다 (Linux에서는 root, macOS에서는 Docker Desktop을 통해 사용자 계정). 이 기본값은 대개 Postgres에는 잘못된 값입니다. `coast ssg import-host-volume`를 통해 들어왔고 `docker-compose up`이 첫 시작 시 이미 볼륨에 `chown`을 적용했다면, 이미 문제가 없는 상태입니다.

## `coast ssg doctor`

`coast ssg doctor`는 현재 프로젝트의 SSG에 대해 실행되는 읽기 전용 검사입니다 (`cwd`의 `Coastfile`에 있는 `[coast].name` 또는 `--working-dir`로 확인). 활성 빌드에서 각 `(service, host-bind)` 쌍마다 하나의 결과를 출력하고, 비밀 추출 관련 결과도 출력합니다([Secrets](SECRETS.md) 참조).

알려진 각 이미지(Postgres, MySQL, MariaDB, MongoDB)에 대해 내장된 UID/GID 테이블을 조회하고, 각 호스트 경로에 대해 `stat(2)`와 비교한 뒤 다음을 출력합니다:

- 소유자가 이미지의 기대값과 일치하면 `ok`.
- 다르면 `warn`. 메시지에는 수정용 `chown` 명령이 포함됩니다.
- 디렉터리가 아직 없거나, 일치하는 이미지가 named volume만 사용하는 경우(호스트 측에서 확인할 것이 없음) `info`.

이미지가 알려진 이미지 테이블에 없는 서비스는 조용히 건너뜁니다. `ghcr.io/baosystems/postgis` 같은 포크는 표시되지 않습니다 -- doctor는 잘못된 경고를 출력하느니 차라리 아무 말도 하지 않는 쪽을 택합니다.

```bash
coast ssg doctor
```

Postgres 디렉터리 소유자가 맞지 않는 경우의 샘플 출력:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

Doctor는 어떤 것도 수정하지 않습니다. 사용자가 호스트 파일시스템에 둔 바이트의 권한은 Coast가 조용히 변경하는 대상이 아닙니다.

## 플랫폼 참고 사항

- **macOS Docker Desktop.** 원시 호스트 경로는 Settings -> Resources -> File Sharing에 등록되어 있어야 합니다. 기본값에는 `/Users`, `/Volumes`, `/private`, `/tmp`가 포함됩니다. macOS에서 `/var/coast-data`는 **기본 목록에 포함되지 않으므로** 새 경로에는 `$HOME/coast-data/...`를 사용하는 것이 좋고, 또는 `/var/coast-data`를 File Sharing에 추가하세요. `/var/lib/docker/volumes/<name>/_data` 형식은 *호스트 경로가 아닙니다* -- Docker가 자체 VM 내부에서 이를 해석하므로 File Sharing 항목 없이도 동작합니다.
- **WSL2.** WSL 네이티브 경로(`~`, `/mnt/wsl/...`)를 권장합니다. `/mnt/c/...`도 동작하지만 Windows 호스트 파일시스템을 연결하는 9P 프로토콜 때문에 느립니다.
- **Linux.** 특별한 함정은 없습니다.

## 라이프사이클

- `coast ssg rm` -- SSG의 외부 DinD 컨테이너를 제거합니다. **볼륨 내용은 그대로 유지되며**, 호스트 bind-mount 내용도 그대로 유지되고, keystore도 그대로 유지됩니다. 동일한 Docker 볼륨을 사용하는 다른 것들도 계속 동작합니다.
- `coast ssg rm --with-data` -- **SSG의 중첩 Docker 데몬 내부에 존재하는** 볼륨(앞에 슬래시가 없는 `name:path` 형식)을 삭제합니다. 호스트 bind mount와 외부 Docker 볼륨은 여전히 건드리지 않습니다 -- Coast의 소유가 아니기 때문입니다.
- `coast ssg build` -- 볼륨을 절대 건드리지 않습니다. 매니페스트와 (`[secrets]`가 선언된 경우) keystore 행만 기록합니다.
- `coast ssg run` / `start` / `restart` -- 호스트 bind-mount 디렉터리가 없으면 생성합니다 (기본 소유권으로 -- [Permissions](#permissions) 참조).

## 참조

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- 볼륨 문법을 포함한 전체 TOML 스키마
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- SSG가 아닌 서비스에 대한 공유, 격리, 스냅샷 시드 볼륨 전략
- [Building](BUILDING.md) -- 매니페스트가 생성되는 위치
- [Lifecycle](LIFECYCLE.md) -- 볼륨이 생성, 중지, 제거되는 시점
- [Secrets](SECRETS.md) -- 파일 주입 비밀은 `~/.coast/ssg/runs/<project>/secrets/<basename>`에 저장되고 내부 서비스에 읽기 전용으로 bind-mount됩니다

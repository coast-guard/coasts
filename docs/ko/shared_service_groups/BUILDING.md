# 공유 서비스 그룹 빌드하기

`coast ssg build`는 프로젝트의 `Coastfile.shared_service_groups`를 파싱하고, 선언된 모든 시크릿을 추출하며, 모든 이미지를 호스트 이미지 캐시로 가져오고, 버전이 지정된 빌드 아티팩트를 `~/.coast/ssg/<project>/builds/<build_id>/` 아래에 기록합니다. 이 명령은 이미 실행 중인 SSG에 대해 비파괴적입니다 -- 다음 `coast ssg run` 또는 `coast ssg start`가 새 빌드를 가져오지만, 실행 중인 `<project>-ssg`는 재시작할 때까지 현재 빌드를 계속 제공합니다.

프로젝트 이름은 같은 디렉터리에 있는 `Coastfile`의 `[coast].name`에서 가져옵니다. 각 프로젝트는 자신만의 `<project>-ssg`라는 SSG, 자신만의 빌드 디렉터리, 그리고 자신만의 `latest_build_id`를 가집니다 -- 호스트 전체에 대한 "현재 SSG"라는 개념은 없습니다.

전체 TOML 스키마는 [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md)를 참조하세요.

## 검색

`coast ssg build`는 `coast build`와 동일한 규칙을 사용하여 Coastfile을 찾습니다:

- 플래그가 없으면 현재 작업 디렉터리에서 `Coastfile.shared_service_groups` 또는 `Coastfile.shared_service_groups.toml`을 찾습니다. 두 형식은 동일하며 둘 다 존재할 경우 `.toml` 접미사가 우선합니다.
- `-f <path>` / `--file <path>`는 임의의 파일 경로를 가리킵니다.
- `--working-dir <dir>`는 프로젝트 루트와 Coastfile 위치를 분리합니다 (`coast build --working-dir`와 동일한 플래그).
- `--config '<inline-toml>'`는 Coastfile을 인라인으로 생성하는 스크립팅 및 CI 흐름을 지원합니다.

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

빌드는 같은 디렉터리에 있는 형제 `Coastfile`에서 프로젝트 이름을 확인합니다. `--config`를 사용하는 경우(디스크 상의 Coastfile.shared_service_groups가 없는 경우)에도 현재 작업 디렉터리에는 `[coast].name`이 SSG 프로젝트인 `Coastfile`이 반드시 있어야 합니다.

## Build가 수행하는 작업

각 `coast ssg build`는 `coast build`와 동일한 `BuildProgressEvent` 채널을 통해 진행 상황을 스트리밍하므로, CLI는 `[N/M]` 단계 카운터를 렌더링합니다.

1. **`Coastfile.shared_service_groups`를 파싱합니다.** 허용되는 최상위 섹션은 `[ssg]`, `[shared_services.*]`, `[secrets.*]`, `[unset]`입니다. 볼륨 항목은 호스트 바인드 마운트와 내부 명명 볼륨으로 분리됩니다([Volumes](VOLUMES.md) 참조).
2. **빌드 id를 확인합니다.** id의 형식은 `{coastfile_hash}_{YYYYMMDDHHMMSS}`입니다. 해시는 원시 소스, 파싱된 서비스의 결정론적 요약, 그리고 `[secrets.*]` 설정을 함께 반영합니다(따라서 시크릿의 `extractor`나 `var`를 수정하면 새로운 id가 생성됩니다).
3. **내부 `compose.yml`을 생성합니다.** 모든 `[shared_services.*]` 블록은 단일 Docker Compose 파일의 항목이 됩니다. 이 파일은 `coast ssg run` 시점에 SSG의 내부 Docker 데몬이 `docker compose up -d`로 실행하는 파일입니다.
4. **시크릿을 추출합니다.** `[secrets.*]`가 비어 있지 않으면, 선언된 각 extractor를 실행하고 암호화된 결과를 `coast_image = "ssg:<project>"` 아래 `~/.coast/keystore.db`에 저장합니다. Coastfile에 `[secrets]` 블록이 없으면 조용히 건너뜁니다. 전체 파이프라인은 [Secrets](SECRETS.md)를 참조하세요.
5. **각 이미지를 가져와 캐시합니다.** 이미지는 `~/.coast/image-cache/`에 OCI tarball로 저장되며, 이는 `coast build`가 사용하는 것과 동일한 풀입니다. 어느 명령에서든 캐시 적중이 발생하면 다른 명령도 더 빨라집니다.
6. **빌드 아티팩트를 작성합니다** `~/.coast/ssg/<project>/builds/<build_id>/`에 세 개의 파일 `manifest.json`, `ssg-coastfile.toml`, `compose.yml`과 함께 작성합니다(아래 레이아웃 참조).
7. **프로젝트의 `latest_build_id`를 업데이트합니다.** 이것은 파일시스템 심볼릭 링크가 아니라 상태 데이터베이스 플래그입니다. `coast ssg run`과 `coast ssg ps`는 어떤 빌드를 대상으로 작업할지 알기 위해 이를 읽습니다.
8. **자동 정리**를 수행하여 이 프로젝트의 이전 빌드를 최근 5개까지만 유지합니다. `~/.coast/ssg/<project>/builds/` 아래의 더 오래된 아티팩트 디렉터리는 디스크에서 제거됩니다. 고정된 빌드(아래의 "프로젝트를 특정 빌드에 고정하기" 참조)는 항상 보존됩니다.

## 아티팩트 레이아웃

```text
~/.coast/
  keystore.db                                          (공유됨, coast_image로 네임스페이스 분리)
  keystore.key
  image-cache/                                         (공유 OCI tarball 풀)
  ssg/
    cg/                                                (프로젝트 "cg")
      builds/
        b455787d95cfdeb_20260420061903/                (새 빌드)
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               (이전 빌드)
          ...
    filemap/                                           (프로젝트 "filemap" -- 별도 트리)
      builds/
        ...
    runs/
      cg/                                              (프로젝트별 실행 스크래치)
        compose.override.yml                           (`coast ssg run` 시 렌더링됨)
        secrets/<basename>                             (파일 주입 시크릿, 모드 0600)
```

`manifest.json`은 다운스트림 코드가 중요하게 여기는 빌드 메타데이터를 담고 있습니다:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

Env 값과 시크릿 페이로드는 의도적으로 포함되지 않습니다 -- env 변수 이름과 주입 *대상*만 기록됩니다. 시크릿 값은 아티팩트 파일이 아니라 keystore에 암호화되어 저장됩니다.

`ssg-coastfile.toml`은 파싱되고, 보간되며, 검증 후의 Coastfile입니다. 바이트 단위로 데몬이 파싱 시점에 보았을 것과 동일합니다. 과거 빌드를 감사할 때 유용합니다.

`compose.yml`은 SSG의 내부 Docker 데몬이 실행하는 파일입니다. 특히 대칭 경로 바인드 마운트 전략을 포함한 생성 규칙은 [Volumes](VOLUMES.md)를 참조하세요.

## 실행하지 않고 빌드 검사하기

`coast ssg ps`는 프로젝트의 `latest_build_id`에 대한 `manifest.json`을 직접 읽습니다 -- 어떤 컨테이너도 검사하지 않습니다. 다음 `coast ssg run`에서 시작될 서비스를 보기 위해 `coast ssg build` 직후 즉시 실행할 수 있습니다:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

`PORT` 열은 내부 컨테이너 포트입니다. 동적 호스트 포트는 `coast ssg run` 시점에 할당되며, 소비자 대상 가상 포트는 `coast ssg ports`에서 보고됩니다. 전체 그림은 [Routing](ROUTING.md)을 참조하세요.

프로젝트의 모든 빌드(타임스탬프, 서비스 수, 현재 어떤 빌드가 최신인지 포함)를 둘러보려면 다음을 사용하세요:

```bash
coast ssg builds-ls
```

## 리빌드

새 `coast ssg build`는 SSG를 업데이트하는 정식 방법입니다. 이 명령은 시크릿이 있으면 다시 추출하고, `latest_build_id`를 업데이트하며, 오래된 아티팩트를 정리합니다. 소비자는 자동으로 리빌드되지 않습니다 -- 소비자의 `from_group = true` 참조는 소비자 빌드 시점에 그때 현재였던 빌드를 기준으로 확인됩니다. 소비자를 더 새로운 SSG로 롤링하려면 소비자에 대해 `coast build`를 실행하세요.

런타임은 리빌드 간에도 유연합니다: 가상 포트는 `(project, service, container_port)`마다 안정적으로 유지되므로, 라우팅을 위해 소비자를 새로 고칠 필요가 없습니다. 형태 변경(서비스 이름이 바뀌었거나 제거됨)은 Coast 수준의 "drift" 메시지가 아니라 소비자 수준의 연결 오류로 드러납니다. 이유는 [Routing](ROUTING.md)을 참조하세요.

## 프로젝트를 특정 빌드에 고정하기

기본적으로 SSG는 프로젝트의 `latest_build_id`를 실행합니다. 이전 빌드에 프로젝트를 고정해야 하는 경우 -- 회귀 재현, worktree 간 두 빌드의 A/B 비교, 또는 장수 브랜치를 검증된 형태에 유지하는 경우 -- pin 명령을 사용하세요:

```bash
coast ssg checkout-build <build_id>     # 이 프로젝트를 <build_id>에 고정
coast ssg show-pin                      # 활성 pin 보고 (있는 경우)
coast ssg uncheckout-build              # pin 해제; latest로 복귀
```

Pin은 소비자 프로젝트별입니다(프로젝트당 하나의 pin, worktree 간 공유). 고정된 경우:

- `coast ssg run`은 `latest_build_id` 대신 고정된 빌드를 자동 시작합니다.
- `coast build`는 고정된 빌드의 manifest에 대해 `from_group` 참조를 검증합니다.
- `auto_prune`는 고정된 빌드 디렉터리가 최근 5개 창 밖에 있더라도 삭제하지 않습니다.

Coastguard SPA는 pin이 활성화된 경우 빌드 id 옆에 `PINNED` 배지를 표시하고, 그렇지 않으면 `LATEST`를 표시합니다. pin 명령은 [CLI](CLI.md)에도 나와 있습니다.

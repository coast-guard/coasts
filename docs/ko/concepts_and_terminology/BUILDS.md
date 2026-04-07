# 빌드

coast 빌드는 추가 지원이 있는 Docker 이미지라고 생각하면 됩니다. 빌드는 Coast 인스턴스를 생성하는 데 필요한 모든 것을 묶어 둔 디렉터리 기반 아티팩트입니다: 해석된 [Coastfile](COASTFILE_TYPES.md), 다시 작성된 compose 파일, 미리 pull된 OCI 이미지 tarball, 그리고 주입된 호스트 파일이 포함됩니다. 이것은 Docker 이미지 자체는 아니지만, Docker 이미지들(tarball 형태)과 Coast가 그것들을 함께 연결하는 데 필요한 메타데이터를 담고 있습니다.

## `coast build`가 하는 일

`coast build`를 실행하면 데몬은 다음 단계를 순서대로 수행합니다:

1. Coastfile을 파싱하고 검증합니다.
2. compose 파일을 읽고 생략된 서비스를 필터링합니다.
3. 설정된 추출기에서 [secrets](SECRETS.md)를 추출하고 암호화하여 keystore에 저장합니다.
4. `build:` 지시어가 있는 compose 서비스에 대해 Docker 이미지를 빌드합니다(호스트에서).
5. `image:` 지시어가 있는 compose 서비스에 대해 Docker 이미지를 pull합니다.
6. 모든 이미지를 `~/.coast/image-cache/`에 OCI tarball로 캐시합니다.
7. `[coast.setup]`가 설정되어 있으면, 지정된 패키지, 명령, 파일로 커스텀 DinD 베이스 이미지를 빌드합니다.
8. manifest, 해석된 coastfile, 다시 작성된 compose, 주입된 파일을 포함하는 빌드 아티팩트 디렉터리를 작성합니다.
9. `latest` 심볼릭 링크를 새 빌드를 가리키도록 업데이트합니다.
10. 보관 한도를 초과한 오래된 빌드를 자동으로 정리합니다.

## 빌드가 저장되는 위치

```text
~/.coast/
  images/
    my-project/
      latest -> a3c7d783_20260227143000       (symlink)
      a3c7d783_20260227143000/                (versioned build)
        manifest.json
        coastfile.toml
        compose.yml
        inject/
      b4d8e894_20260226120000/                (older build)
        ...
  image-cache/                                (shared tarball cache)
    postgres_16_a1b2c3d4e5f6.tar
    redis_7_f6e5d4c3b2a1.tar
    coast-built_my-project_web_latest_...tar
```

각 빌드는 `{coastfile_hash}_{YYYYMMDDHHMMSS}` 형식의 고유한 **build ID**를 가집니다. 해시는 Coastfile 내용과 해석된 구성을 포함하므로, Coastfile이 변경되면 새로운 build ID가 생성됩니다.

`latest` 심볼릭 링크는 빠른 해석을 위해 항상 가장 최근 빌드를 가리킵니다. 프로젝트가 타입이 지정된 Coastfile(예: `Coastfile.light`)을 사용하는 경우, 각 타입은 자체 심볼릭 링크를 가집니다: `latest-light`.

`~/.coast/image-cache/`의 이미지 캐시는 모든 프로젝트에서 공유됩니다. 두 프로젝트가 같은 Postgres 이미지를 사용하면, tarball은 한 번만 캐시됩니다.

## 빌드에 포함되는 것

각 빌드 디렉터리에는 다음이 포함됩니다:

- **`manifest.json`** -- 프로젝트 이름, 빌드 타임스탬프, coastfile 해시, 캐시되거나 빌드된 이미지 목록, 시크릿 이름, 생략된 서비스, [볼륨 전략](VOLUMES.md) 등 전체 빌드 메타데이터.
- **`coastfile.toml`** -- 해석된 Coastfile(`extends`를 사용하는 경우 부모와 병합됨).
- **`compose.yml`** -- `build:` 지시어가 미리 빌드된 이미지 태그로 대체되고, 생략된 서비스가 제거된 compose 파일의 재작성 버전.
- **`inject/`** -- `[inject].files`의 호스트 파일 복사본(예: `~/.gitconfig`, `~/.npmrc`).

## 빌드에는 시크릿이 포함되지 않습니다

시크릿은 빌드 단계에서 추출되지만, 빌드 아티팩트 디렉터리 내부가 아니라 `~/.coast/keystore.db`의 별도 암호화된 keystore에 저장됩니다. manifest에는 추출된 시크릿의 **이름**만 기록되며, 값은 절대 기록되지 않습니다.

이는 민감한 데이터를 노출하지 않고도 빌드 아티팩트를 안전하게 검사할 수 있음을 의미합니다. 시크릿은 이후 `coast run`으로 Coast 인스턴스를 생성할 때 복호화되고 주입됩니다.

## 빌드와 Docker

빌드에는 세 종류의 Docker 이미지가 관련됩니다:

- **빌드된 이미지** -- `build:` 지시어가 있는 compose 서비스는 호스트에서 `docker build`를 통해 빌드되고, `coast-built/{project}/{service}:latest`로 태그되며, 이미지 캐시에 tarball로 저장됩니다.
- **pull된 이미지** -- `image:` 지시어가 있는 compose 서비스는 pull되어 tarball로 저장됩니다.
- **Coast 이미지** -- `[coast.setup]`가 설정되어 있으면, 지정된 패키지, 명령, 파일을 사용해 `docker:dind` 위에 커스텀 Docker 이미지가 빌드됩니다. `coast-image/{project}:{build_id}`로 태그됩니다.

런타임 시([`coast run`](RUN.md)) 이 tarball들은 `docker load`를 통해 내부 [DinD daemon](RUNTIMES_AND_SERVICES.md)에 로드됩니다. 이것이 Coast 인스턴스가 레지스트리에서 이미지를 pull할 필요 없이 빠르게 시작되는 이유입니다.

## 빌드와 인스턴스

[`coast run`](RUN.md)을 실행하면 Coast는 최신 빌드(또는 특정 `--build-id`)를 해석하고, 그 아티팩트를 사용하여 인스턴스를 생성합니다. build ID는 인스턴스에 기록됩니다.

더 많은 인스턴스를 생성하기 위해 다시 빌드할 필요는 없습니다. 하나의 빌드가 병렬로 실행되는 많은 Coast 인스턴스를 지원할 수 있습니다.

## 다시 빌드해야 하는 시점

Coastfile, `docker-compose.yml`, 또는 인프라 구성이 변경될 때만 다시 빌드하세요. 다시 빌드하는 작업은 리소스를 많이 사용합니다 -- 이미지를 다시 pull하고, Docker 이미지를 다시 빌드하며, 시크릿을 다시 추출합니다.

코드 변경은 다시 빌드할 필요가 없습니다. Coast는 프로젝트 디렉터리를 각 인스턴스에 직접 마운트하므로, 코드 업데이트가 즉시 반영됩니다.

## 자동 정리

Coast는 Coastfile 타입별로 최대 5개의 빌드를 유지합니다. 성공적인 `coast build`가 실행될 때마다, 한도를 초과한 오래된 빌드는 자동으로 제거됩니다.

실행 중인 인스턴스에서 사용 중인 빌드는 한도와 관계없이 절대 정리되지 않습니다. 빌드가 7개 있고 그중 3개가 활성 인스턴스를 지원하고 있다면, 그 3개는 모두 보호됩니다.

## 수동 제거

`coast rm-build` 또는 Coastguard Builds 탭을 통해 빌드를 수동으로 제거할 수 있습니다.

- **전체 프로젝트 제거** (`coast rm-build <project>`)는 먼저 모든 인스턴스를 중지하고 제거해야 합니다. 이 작업은 전체 빌드 디렉터리, 관련 Docker 이미지, 볼륨, 컨테이너를 제거합니다.
- **선택적 제거** (Coastguard UI에서 build ID 기준으로 사용 가능)는 실행 중인 인스턴스에서 사용 중인 빌드를 건너뜁니다.

## 타입별 빌드

프로젝트가 여러 Coastfile(예: 기본 구성을 위한 `Coastfile`, 스냅샷 시드 볼륨을 위한 `Coastfile.snap`)을 사용하는 경우, 각 타입은 자체 `latest-{type}` 심볼릭 링크와 자체 5개 빌드 정리 풀을 유지합니다.

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

`snap` 빌드를 정리해도 `default` 빌드에는 전혀 영향을 주지 않으며, 그 반대도 마찬가지입니다.

## 원격 빌드

[remote coast](REMOTES.md)에 대해 빌드할 때, 빌드는 `coast-service`를 통해 원격 머신에서 실행되므로 이미지가 원격 환경의 네이티브 아키텍처를 사용합니다. 그런 다음 아티팩트는 재사용을 위해 로컬 머신으로 다시 전송됩니다. 원격 빌드는 자체 `latest-remote` 심볼릭 링크를 유지하며 아키텍처별로 정리됩니다. 자세한 내용은 [Remotes](REMOTES.md)를 참조하세요.

# `coast ssg` CLI 참조

모든 `coast ssg` 하위 명령은 기존 Unix 소켓을 통해 동일한 로컬 데몬과 통신합니다. `coast shared-service-group`는 `coast ssg`의 별칭입니다.

대부분의 동사는 cwd `Coastfile`의 `[coast].name`(또는 `--working-dir <dir>`)에서 프로젝트를 확인합니다. `coast ssg ls`만 프로젝트 간 조회입니다.

모든 명령은 진행 출력를 억제하고 최종 요약 또는 오류만 출력하는 전역 `--silent` / `-s` 플래그를 허용합니다.

## 명령어

### 빌드 및 검사

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | `Coastfile.shared_service_groups`를 파싱하고, 모든 `[secrets.*]`를 추출하며, 이미지를 pull하고, 산출물을 `~/.coast/ssg/<project>/builds/<id>/`에 작성하고, `latest_build_id`를 업데이트하며, 오래된 빌드를 정리합니다. [빌드](BUILDING.md)를 참조하세요. |
| `coast ssg ps` | 이 프로젝트의 SSG 빌드 서비스 목록을 표시합니다(`manifest.json` 및 라이브 컨테이너 상태를 읽음). [라이프사이클 -> ps](LIFECYCLE.md#coast-ssg-ps)를 참조하세요. |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | `~/.coast/ssg/<project>/builds/` 아래의 모든 빌드 산출물을 타임스탬프, 서비스 수, `(latest)` / `(pinned)` 주석과 함께 나열합니다. |
| `coast ssg ls` | 데몬이 알고 있는 모든 SSG를 프로젝트 간으로 나열합니다(프로젝트, 상태, 빌드 id, 서비스 수, 생성 시각). [라이프사이클 -> ls](LIFECYCLE.md#coast-ssg-ls)를 참조하세요. |

### 라이프사이클

| Command | Summary |
|---------|---------|
| `coast ssg run` | `<project>-ssg` DinD를 생성하고, 동적 호스트 포트를 할당하며, 비밀 정보를 구체화하고(선언된 경우), 내부 compose 스택을 부팅합니다. [라이프사이클 -> run](LIFECYCLE.md#coast-ssg-run)을 참조하세요. |
| `coast ssg start` | 이전에 생성되었지만 중지된 SSG를 시작합니다. 비밀 정보를 다시 구체화하고 보존된 canonical-port checkout socat를 다시 생성합니다. |
| `coast ssg stop [--force]` | 프로젝트의 SSG DinD를 중지합니다. 컨테이너, 동적 포트, 가상 포트, checkout 행을 보존합니다. `--force`는 먼저 원격 SSH 터널을 해제합니다. |
| `coast ssg restart` | 중지 후 시작합니다. 컨테이너와 동적 포트를 보존합니다. |
| `coast ssg rm [--with-data] [--force]` | 프로젝트의 SSG DinD를 제거합니다. `--with-data`는 내부 named volume을 삭제합니다. `--force`는 원격 shadow 소비자가 있어도 진행합니다. 호스트 bind-mount 내용은 절대 건드리지 않습니다. **Keystore는 절대 건드리지 않습니다** -- 이 작업에는 `coast ssg secrets clear`를 사용하세요. |

### 로그 및 exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | 외부 DinD 또는 하나의 내부 서비스에서 로그를 스트리밍합니다. `--follow`는 Ctrl+C까지 스트리밍합니다. |
| `coast ssg exec [--service <name>] -- <cmd...>` | 외부 `<project>-ssg` 컨테이너 또는 하나의 내부 서비스에 exec합니다. `--` 뒤의 모든 내용은 그대로 전달됩니다. |

### 라우팅 및 checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | 해당하는 경우 `(checked out)` 주석과 함께 서비스별 canonical / dynamic / virtual 포트 매핑을 표시합니다. [라우팅](ROUTING.md)을 참조하세요. |
| `coast ssg checkout [--service <name> \| --all]` | 호스트 측 socat를 통해 canonical 호스트 포트를 바인드합니다(포워더 대상은 프로젝트의 안정적인 virtual 포트입니다). 경고와 함께 Coast 인스턴스 보유자를 밀어내며, 알 수 없는 호스트 프로세스에는 오류를 반환합니다. [체크아웃](CHECKOUT.md)을 참조하세요. |
| `coast ssg uncheckout [--service <name> \| --all]` | 이 프로젝트의 canonical-port socat를 해제합니다. 밀려난 Coast를 자동으로 복원하지는 않습니다. |

### 진단

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | 알려진 이미지 서비스와 선언되었지만 추출되지 않은 SSG 비밀 정보에 대해 호스트 bind-mount 권한 전반을 읽기 전용으로 검사합니다. `ok` / `warn` / `info` 결과를 출력합니다. [볼륨 -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor)를 참조하세요. |

### 빌드 고정

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | 이 프로젝트의 SSG를 특정 `build_id`에 고정합니다. `coast ssg run` 및 `coast build`는 `latest_build_id` 대신 이 고정을 사용합니다. [빌드 -> 프로젝트를 특정 빌드에 고정하기](BUILDING.md#locking-a-project-to-a-specific-build)를 참조하세요. |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | 고정을 해제합니다. 멱등적입니다. |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | 있다면 이 프로젝트의 현재 고정을 표시합니다. |

### SSG 네이티브 비밀 정보

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | `coast_image = "ssg:<project>"` 아래의 모든 암호화된 keystore 항목을 삭제합니다. 멱등적입니다. SSG 네이티브 비밀 정보를 지우는 유일한 동사입니다 -- `coast ssg rm` 및 `rm --with-data`는 의도적으로 이를 그대로 둡니다. [비밀 정보](SECRETS.md)를 참조하세요. |

### 마이그레이션 도우미

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | 호스트 Docker named volume의 마운트 지점을 확인하고 동등한 SSG bind-mount 항목을 출력(또는 적용)합니다. [볼륨 -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume)을 참조하세요. |

## 종료 코드

- `0` -- 성공. `doctor` 같은 명령은 경고를 발견하더라도 0을 반환합니다. 이는 게이트가 아니라 진단 도구입니다.
- 0이 아님 -- 검증 오류, Docker 오류, 상태 불일치 또는 remote-shadow 게이트 거부.

## 함께 보기

- [빌드](BUILDING.md)
- [라이프사이클](LIFECYCLE.md)
- [라우팅](ROUTING.md)
- [볼륨](VOLUMES.md)
- [소비](CONSUMING.md)
- [비밀 정보](SECRETS.md)
- [체크아웃](CHECKOUT.md)

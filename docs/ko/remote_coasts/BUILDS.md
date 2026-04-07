# 원격 빌드

원격 빌드는 coast-service를 통해 원격 머신에서 실행됩니다. 이를 통해 로컬 아키텍처(예: ARM Mac)와 관계없이 빌드가 원격의 네이티브 아키텍처(예: EC2 인스턴스의 x86_64)를 사용하도록 보장합니다. 크로스 컴파일이나 아키텍처 에뮬레이션이 필요하지 않습니다.

## 동작 방식

`coast build --type remote`를 실행하면 다음이 발생합니다:

1. 데몬이 프로젝트 소스 파일(Coastfile, compose.yml, Dockerfiles, inject/)을 SSH를 통해 원격 작업 공간으로 rsync합니다.
2. 데몬이 SSH 터널을 통해 coast-service에 `POST /build`를 호출합니다.
3. coast-service가 원격에서 전체 빌드를 네이티브로 실행합니다: `docker build`, 이미지 가져오기, 이미지 캐싱, 비밀 추출을 모두 `/data/images/` 아래에서 수행합니다.
4. coast-service가 아티팩트 경로와 빌드 메타데이터를 포함한 `BuildResponse`를 반환합니다.
5. 데몬이 전체 아티팩트 디렉터리(coastfile.toml, compose.yml, manifest.json, secrets/, inject/, 이미지 tarball)를 로컬 머신의 `~/.coast/images/{project}/{build_id}/`로 다시 rsync합니다.
6. 데몬이 새 빌드를 가리키는 `latest-remote` 심볼릭 링크를 생성합니다.

```text
Local Machine                              Remote Machine
┌─────────────────────────────┐            ┌───────────────────────────┐
│  ~/.coast/images/my-app/    │            │  /data/images/my-app/     │
│    latest-remote -> {id}    │  ◀─rsync─  │    {id}/                  │
│    {id}/                    │            │      manifest.json        │
│      manifest.json          │            │      coastfile.toml       │
│      coastfile.toml         │            │      compose.yml          │
│      compose.yml            │            │      *.tar (images)       │
│      *.tar (images)         │            │                           │
└─────────────────────────────┘            └───────────────────────────┘
```

## 명령어

```bash
# Build on the default remote (auto-selected if only one registered)
coast build --type remote

# Build on a specific remote
coast build --type remote --remote my-vm

# Build without running (standalone)
coast build --type remote
```

호환되는 빌드가 아직 없으면 `coast run --type remote`도 빌드를 트리거합니다.

## 아키텍처 일치

각 빌드의 `manifest.json`에는 해당 빌드가 어떤 아키텍처용으로 생성되었는지(예: `aarch64`, `x86_64`)가 기록됩니다. `coast run --type remote`를 실행하면 데몬은 기존 빌드가 대상 원격의 아키텍처와 일치하는지 확인합니다:

- **아키텍처가 일치함**: 빌드를 재사용합니다. 다시 빌드할 필요가 없습니다.
- **아키텍처가 일치하지 않음**: 데몬이 올바른 아키텍처를 가진 가장 최신 빌드를 찾습니다. 없으면 다시 빌드하라는 안내와 함께 오류를 반환합니다.

즉, x86_64 원격에서 한 번 빌드하면 다시 빌드하지 않고도 얼마든지 많은 x86_64 원격에 배포할 수 있습니다. 하지만 ARM 빌드를 x86_64 원격에서 사용하거나 그 반대로 사용하는 것은 불가능합니다.

## 심볼릭 링크

원격 빌드는 로컬 빌드와 별도의 심볼릭 링크를 사용합니다:

| Symlink | Points to |
|---------|-----------|
| `latest` | 가장 최근의 로컬 빌드 |
| `latest-remote` | 가장 최근의 원격 빌드 |
| `latest-{type}` | 특정 Coastfile 타입의 가장 최근 로컬 빌드 |

이 분리는 원격 빌드가 로컬 `latest` 심볼릭 링크를 덮어쓰거나 그 반대로 되는 것을 방지합니다.

## 자동 정리

Coast는 `(coastfile_type, architecture)` 쌍마다 최대 5개의 원격 빌드를 유지합니다. 매번 원격 빌드가 성공한 후 제한을 초과하는 오래된 빌드는 자동으로 제거됩니다.

실행 중인 인스턴스에서 사용 중인 빌드는 제한과 관계없이 절대 정리되지 않습니다. x86_64 원격 빌드가 7개 있고 그중 3개가 활성 인스턴스를 뒷받침하고 있다면, 그 3개는 모두 보호됩니다.

정리는 아키텍처를 인식합니다: `aarch64` 및 `x86_64` 원격 빌드가 모두 있는 경우, 각 아키텍처는 독립적으로 자체 5개 빌드 풀을 유지합니다.

## 아티팩트 저장소

원격 빌드 아티팩트는 두 위치에 저장됩니다:

| Location | Path | Purpose |
|----------|------|---------|
| Remote | `/data/images/{project}/{build_id}/` | 원격 머신의 source of truth |
| Local | `~/.coast/images/{project}/{build_id}/` | 원격 간 재사용을 위한 로컬 캐시 |

원격의 `/data/image-cache/`에 있는 이미지 캐시는 로컬의 `~/.coast/image-cache/`와 마찬가지로 모든 프로젝트에서 공유됩니다.

## 로컬 빌드와의 관계

원격 빌드와 로컬 빌드는 서로 독립적입니다. `coast build` (`--type remote` 없이)는 항상 로컬 머신에서 빌드하고 `latest` 심볼릭 링크를 업데이트합니다. `coast build --type remote`는 항상 원격 머신에서 빌드하고 `latest-remote` 심볼릭 링크를 업데이트합니다.

동일한 프로젝트에 대해 로컬 빌드와 원격 빌드가 함께 공존할 수 있습니다. 로컬 coast는 로컬 빌드를 사용하고, 원격 coast는 원격 빌드를 사용합니다.

일반적인 빌드 동작 방식(매니페스트 구조, 이미지 캐싱, 타입별 빌드)에 대한 자세한 내용은 [Builds](../concepts_and_terminology/BUILDS.md)를 참조하세요.

# 리모트

리모트 coast는 노트북 대신 원격 머신에서 서비스를 실행합니다. CLI 및 UI 경험은 로컬 coast와 동일합니다 -- `coast run`, `coast assign`, `coast exec`, `coast ps`, `coast checkout`는 모두 같은 방식으로 동작합니다. 데몬은 인스턴스가 리모트임을 감지하고 원격 호스트의 `coast-service`로 SSH 터널을 통해 작업을 라우팅합니다.

## 로컬 vs 리모트

| | 로컬 Coast | 리모트 Coast |
|---|---|---|
| DinD 컨테이너 | 사용자 머신에서 실행됨 | 원격 머신에서 실행됨 |
| Compose 서비스 | 로컬 DinD 내부 | 리모트 DinD 내부 |
| 파일 편집 | 직접 바인드 마운트 | 셸 coast(로컬) + rsync/mutagen 동기화 |
| 포트 접근 | `socat` 포워더 | SSH `-L` 터널 + `socat` 포워더 |
| 공유 서비스 | 브리지 네트워크 | SSH `-R` 리버스 터널 |
| 빌드 아키텍처 | 사용자 머신의 아키텍처 | 원격 머신의 아키텍처 |

## 작동 방식

모든 리모트 coast는 두 개의 컨테이너를 생성합니다:

1. 로컬 머신의 **셸 coast**. 이것은 일반 coast와 동일한 바인드 마운트(``/host-project`, `/workspace`)를 가진 경량 Docker 컨테이너(`sleep infinity`)입니다. 이 컨테이너는 호스트 에이전트가 원격으로 동기화되는 파일을 편집할 수 있도록 존재합니다.

2. `coast-service`가 관리하는 원격 머신의 **리모트 coast**. 이것은 동적 포트를 사용하여 실제 DinD 컨테이너와 compose 서비스를 실행합니다.

데몬은 SSH 터널로 둘을 연결합니다:

- **포워드 터널** (`ssh -L`): 각 로컬 동적 포트를 해당하는 원격 동적 포트에 매핑하여 `localhost:{dynamic}`가 원격 서비스에 도달하도록 합니다.
- **리버스 터널** (`ssh -R`): 로컬 [공유 서비스](SHARED_SERVICES.md) (Postgres, Redis)를 원격 DinD 컨테이너에 노출합니다.

## 리모트 등록

리모트는 데몬에 등록되며 `state.db`에 저장됩니다:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/coast_key
coast remote test my-vm
coast remote ls
coast remote rm my-vm
```

연결 세부 정보(호스트, 사용자, 포트, SSH 키)는 Coastfile이 아니라 데몬의 데이터베이스에 저장됩니다. Coastfile은 `[remote]` 섹션을 통해 동기화 기본 설정만 선언합니다.

## 리모트 빌드

빌드는 원격 머신에서 수행되므로 이미지가 원격의 네이티브 아키텍처를 사용합니다. ARM Mac은 크로스 컴파일 없이 x86_64 리모트에서 x86_64 이미지를 빌드할 수 있습니다.

빌드 후 아티팩트는 재사용을 위해 로컬 머신으로 다시 전송됩니다. 다른 리모트가 동일한 아키텍처를 가지고 있다면, 사전 빌드된 아티팩트를 다시 빌드하지 않고 직접 배포할 수 있습니다. 빌드 아티팩트가 어떻게 구성되는지에 대한 자세한 내용은 [빌드](BUILDS.md)를 참조하세요.

## 파일 동기화

리모트 coast는 초기 대량 전송에는 rsync를 사용하고, 지속적인 실시간 동기화에는 mutagen을 사용합니다. 두 도구 모두 호스트 머신이 아니라 coast 컨테이너(셸 coast 및 coast-service 이미지) 내부에서 실행됩니다. 동기화 구성에 대한 자세한 내용은 [리모트 Coasts](../remote_coasts/README.md) 가이드를 참조하세요.

## 디스크 관리

원격 머신에는 Docker 볼륨, 워크스페이스 디렉터리, 이미지 tarball이 축적됩니다. `coast rm`으로 리모트 인스턴스를 제거하면 연결된 모든 리소스가 정리됩니다. 실패한 작업에서 남은 고아 리소스는 `coast remote prune`를 사용하세요.

## 설정

호스트 요구 사항, coast-service 배포, Coastfile 구성 등을 포함한 전체 설정 지침은 [리모트 Coasts](../remote_coasts/README.md) 가이드를 참조하세요.

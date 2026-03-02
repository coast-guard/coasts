# 문제 해결

Coasts에서 발생하는 대부분의 문제는 오래된 상태(stale state), 고아가 된 Docker 리소스, 또는 동기화가 어긋난 데몬 때문에 생깁니다. 이 페이지는 가벼운 조치부터 핵 옵션까지의 에스컬레이션 경로를 다룹니다.

## Doctor

뭔가 이상하다고 느껴진다면 — 인스턴스가 실행 중으로 표시되지만 아무것도 응답하지 않거나, 포트가 막힌 것처럼 보이거나, UI가 오래된 데이터를 보여주는 경우 — `coast doctor`부터 시작하세요:

```bash
coast doctor
```

Doctor는 상태 데이터베이스와 Docker를 스캔하여 불일치를 찾습니다: 컨테이너가 누락된 고아 인스턴스 레코드, 상태 레코드가 없는 떠다니는(dangling) 컨테이너, 그리고 실행 중으로 표시되어 있지만 실제로는 죽어 있는 공유 서비스 등이 포함됩니다. 발견한 문제는 자동으로 수정합니다.

아무것도 변경하지 않고 어떤 작업을 할지 미리 확인하려면:

```bash
coast doctor --dry-run
```

## Daemon Restart

데몬 자체가 응답하지 않는 것 같거나 상태가 꼬였다고 의심된다면, 데몬을 재시작하세요:

```bash
coast daemon restart
```

이 명령은 정상 종료 신호를 보내고 데몬이 종료될 때까지 기다린 다음, 새 프로세스를 시작합니다. 인스턴스와 상태는 유지됩니다.

## Removing a Single Project

문제가 하나의 프로젝트에만 국한되어 있다면, 다른 것에는 영향을 주지 않고 해당 프로젝트의 빌드 아티팩트와 연관된 Docker 리소스를 제거할 수 있습니다:

```bash
coast rm-build my-project
```

이는 프로젝트의 아티팩트 디렉터리, Docker 이미지, 볼륨, 컨테이너를 삭제합니다. 먼저 확인을 요청합니다. 프롬프트를 건너뛰려면 `--force`를 전달하세요.

## Factory Reset with Nuke

다른 방법이 모두 실패했거나 — 또는 완전히 깨끗한 상태로 초기화하고 싶다면 — `coast nuke`가 전체 공장 초기화를 수행합니다:

```bash
coast nuke
```

이 작업은 다음을 수행합니다:

1. `coastd` 데몬을 중지합니다.
2. coast가 관리하는 Docker 컨테이너 **전부**를 제거합니다.
3. coast가 관리하는 Docker 볼륨 **전부**를 제거합니다.
4. coast가 관리하는 Docker 네트워크 **전부**를 제거합니다.
5. coast Docker 이미지 **전부**를 제거합니다.
6. 전체 `~/.coast/` 디렉터리(상태 데이터베이스, 빌드, 로그, 시크릿, 이미지 캐시)를 삭제합니다.
7. `~/.coast/`를 다시 생성하고 데몬을 재시작하여 coast를 즉시 다시 사용할 수 있게 합니다.

이 작업은 모든 것을 파괴하므로, 확인 프롬프트에서 `nuke`를 입력해야 합니다:

```text
$ coast nuke
WARNING: This will permanently destroy ALL coast data:

  - Stop the coastd daemon
  - Remove all coast-managed Docker containers
  - Remove all coast-managed Docker volumes
  - Remove all coast-managed Docker networks
  - Remove all coast Docker images
  - Delete ~/.coast/ (state DB, builds, logs, secrets, image cache)

Type "nuke" to confirm:
```

프롬프트를 건너뛰려면 `--force`를 전달하세요(스크립트에서 유용함):

```bash
coast nuke --force
```

nuke 이후 coast는 사용할 준비가 되어 있습니다 — 데몬이 실행 중이고 홈 디렉터리가 존재합니다. 프로젝트를 다시 `coast build`하고 `coast run`하기만 하면 됩니다.

## Reporting Bugs

위의 어떤 방법으로도 해결되지 않는 문제를 겪는다면, 보고 시 데몬 로그를 포함하세요:

```bash
coast daemon logs
```

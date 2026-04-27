# SSG 호스트 측 체크아웃

Consumer Coast는 데몬의 라우팅 레이어를 통해 SSG 서비스에 도달합니다(in-DinD socat -> host socat -> dynamic port). 이것은 앱 컨테이너에는 매우 잘 작동합니다. 하지만 서비스가 바로 거기에 있는 것처럼 `localhost:5432`에 연결하려는 호스트 측 호출자 -- MCP, 임시 `psql` 세션, 에디터의 데이터베이스 인스펙터 -- 에는 도움이 되지 않습니다.

`coast ssg checkout`은 이 문제를 해결합니다. 이것은 정규 호스트 포트(Postgres는 5432, Redis는 6379, ...)에 바인드하고 프로젝트의 안정적인 가상 포트로 전달하는 호스트 수준의 socat을 생성합니다. 그 다음에는 호스트의 기존 가상 포트 socat이 트래픽을 SSG의 현재 공개된 동적 포트로 계속 전달합니다.

전체 동작은 프로젝트별입니다. `coast ssg checkout --service postgres`는 cwd의 `Coastfile`을 소유한 프로젝트로 해석됩니다. 이 머신에 두 개의 프로젝트가 있다면, 한 번에 하나만 정규 포트 5432를 점유할 수 있습니다.

## Usage

```bash
coast ssg checkout --service postgres     # bind one service
coast ssg checkout --all                  # bind every SSG service
coast ssg uncheckout --service postgres   # tear down one
coast ssg uncheckout --all                # tear down every active checkout
```

체크아웃이 성공하면 `coast ssg ports`는 바인드된 각 서비스에 `(checked out)`을 주석으로 표시합니다:

```text
  SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
  postgres             5432            54201           42000      (checked out)
  redis                6379            54202           42001
```

Consumer Coast는 호스트 측 체크아웃 상태와 관계없이 항상 in-DinD socat -> virtual port 체인을 통해 SSG 서비스에 도달합니다. 체크아웃은 순전히 호스트 측 편의 기능입니다.

## Two-Hop Forwarder

체크아웃 socat은 SSG의 동적 호스트 포트를 직접 가리키는 것이 **아닙니다**. 그것은 프로젝트의 안정적인 가상 포트를 가리킵니다:

```text
host process            -> 127.0.0.1:5432           (checkout socat, listens here)
                        -> 127.0.0.1:42000          (project's virtual port)
                        -> 127.0.0.1:54201          (SSG's current dynamic port)
                        -> <project>-ssg postgres   (inner service)
```

이 2단계 체인은 동적 포트가 바뀌더라도 체크아웃 socat이 SSG 재빌드 전반에서 계속 작동함을 의미합니다. 업데이트되는 것은 호스트의 virtual-port socat뿐이며, canonical-port socat은 이를 알지 못합니다. 호스트 socat 레이어가 어떻게 유지되는지는 [Routing](ROUTING.md)을 참고하세요.

## Displacement of Coast-Instance Holders

SSG에 정규 포트 체크아웃을 요청할 때, 해당 포트는 이미 점유되어 있을 수 있습니다. 의미론은 누가 점유하고 있는지에 따라 달라집니다:

- **명시적으로 체크아웃된 Coast 인스턴스.** 오늘 조금 전에 어떤 Coast에서 `coast checkout <instance>`가 `localhost:5432`를 그 Coast의 내부 Postgres에 바인드했습니다. SSG 체크아웃은 이것을 **밀어냅니다**: 데몬은 기존 socat을 종료하고, 해당 Coast의 `port_allocations.socat_pid`를 비우고, 대신 SSG의 socat을 바인드합니다. CLI는 명확한 경고를 출력합니다:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432 -> virtual 42000.
  ```

  밀려난 Coast는 나중에 `coast ssg uncheckout`을 하더라도 **자동으로 다시 바인드되지 않습니다**. 그 동적 포트는 여전히 작동하지만, 정규 포트는 `coast checkout my-app/dev-2`를 다시 실행할 때까지 바인드되지 않은 상태로 남습니다.

- **다른 프로젝트의 SSG 체크아웃.** `filemap-ssg`가 이미 5432를 체크아웃한 상태에서 `cg-ssg`의 5432를 체크아웃하려 하면, 데몬은 점유자를 명시하는 명확한 메시지와 함께 이를 거부합니다. 먼저 `filemap-ssg`의 5432를 uncheckout하세요.

- **죽은 `socat_pid`를 가진 이전 SSG 체크아웃 행.** 충돌한 데몬이나 stop/start 사이클에서 남은 오래된 메타데이터입니다. 새 체크아웃은 조용히 그 행을 다시 점유합니다.

- **그 외 무엇이든** (수동으로 시작한 호스트 Postgres, 다른 데몬, 포트 8080의 `nginx`). `coast ssg checkout`은 오류를 반환합니다:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  `--force` 플래그는 없습니다. 알 수 없는 프로세스를 조용히 종료하는 것은 너무 위험하다고 판단되었습니다.

## Stop / Start Behavior

`coast ssg stop`은 실행 중인 정규 포트 socat 프로세스를 종료하지만 **체크아웃 행 자체는 상태 DB에 보존합니다**.

`coast ssg run` / `start` / `restart`는 보존된 행들을 순회하며 각 행마다 새로운 정규 포트 socat을 다시 생성합니다. 정규 포트(5432)는 동일하게 유지되고, `run` 사이클 사이에서 바뀌는 것은 동적 포트뿐입니다. 그리고 체크아웃 socat은 **virtual** 포트(이 역시 안정적임)를 대상으로 하기 때문에, 재바인드는 기계적인 작업입니다.

서비스가 재빌드된 SSG에서 사라지면, 해당 체크아웃 행은 run 응답에서 경고와 함께 제거됩니다:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm`은 프로젝트의 모든 `ssg_port_checkouts` 행을 삭제합니다. `rm`은 설계상 파괴적입니다 -- 명시적으로 깨끗한 상태를 요청했기 때문입니다.

## Daemon Restart Recovery

예상치 못한 데몬 재시작(crash, `coastd restart`, reboot) 후에는 `restore_running_state`가 `ssg_port_checkouts` 테이블을 조회하고 현재의 dynamic / virtual port 할당에 대해 모든 행을 다시 생성합니다. 데몬 변동이 있어도 `localhost:5432`는 계속 바인드된 상태를 유지합니다.

## When to Check Out

- 프로젝트의 SSG Postgres를 GUI 데이터베이스 클라이언트로 가리키고 싶을 때.
- 동적 포트를 먼저 찾지 않고도 `psql "postgres://coast:coast@localhost:5432/mydb"`가 작동하길 원할 때.
- 호스트의 MCP가 안정적인 정규 엔드포인트를 필요로 할 때.
- Coastguard가 SSG의 HTTP 관리자 포트를 프록시하려 할 때.

체크아웃하면 **안 되는** 경우:

- consumer Coast 내부에서의 연결성용 -- 이것은 이미 in-DinD socat에서 virtual port를 통해 작동합니다.
- `coast ssg ports` 출력을 사용하고 도구에 동적 포트를 넣는 것으로 충분할 때.

## See Also

- [Routing](ROUTING.md) -- 정규 / 동적 / 가상 포트 개념과 전체 호스트 측 포워더 체인
- [Lifecycle](LIFECYCLE.md) -- stop / start / rm 세부사항
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- 이 아이디어의 Coast 인스턴스 버전
- [Ports](../concepts_and_terminology/PORTS.md) -- 전체 시스템 전반의 정규 포트와 동적 포트 배선

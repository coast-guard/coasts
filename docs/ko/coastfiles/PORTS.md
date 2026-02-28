# 포트

`[ports]` 섹션은 Coast가 Coast 인스턴스와 호스트 머신 간 포워딩을 위해 관리하는 포트를 선언합니다. 선택 사항인 `[egress]` 섹션은 Coast 인스턴스가 외부로 나가기 위해 접근해야 하는 호스트의 포트를 선언합니다.

런타임에서 포트 포워딩이 동작하는 방식 — 정규(canonical) 포트 vs 동적(dynamic) 포트, 체크아웃 전환, socat — 에 대해서는 [Ports](../concepts_and_terminology/PORTS.md) 및 [Checkout](../concepts_and_terminology/CHECKOUT.md)를 참고하세요.

## `[ports]`

`logical_name = port_number` 형태의 평평한 맵입니다. 각 항목은 Coast 인스턴스가 실행될 때 해당 포트에 대한 포트 포워딩을 설정하도록 Coast에 지시합니다.

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

각 인스턴스는 선언된 각 포트마다 동적 포트(상위 범위, 항상 접근 가능)를 할당받습니다. 또한 [체크아웃된](../concepts_and_terminology/CHECKOUT.md) 인스턴스는 정규 포트(선언한 번호)도 호스트로 포워딩됩니다.

규칙:

- 포트 값은 0이 아닌 부호 없는 16비트 정수(1-65535)여야 합니다.
- 논리 이름은 `coast ports`, Coastguard, `primary_port`에서 식별자로 사용되는 자유 형식 문자열입니다.

### 최소 예제

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 3000
```

### 멀티 서비스 예제

```toml
[ports]
web = 3000
api = 4000
backend = 8080
postgres = 5432
redis = 6379
```

## `primary_port`

`[coast]` 섹션에 설정되며([Project and Setup](PROJECT.md)에 문서화됨), `primary_port`는 [Coastguard](../concepts_and_terminology/COASTGUARD.md)에서 빠른 링크와 서브도메인 라우팅을 위해 선언된 포트 중 하나의 이름을 지정합니다.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

값은 `[ports]`의 키 중 하나와 일치해야 합니다. 자세한 내용은 [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md)를 참고하세요.

## `[egress]`

Coast 인스턴스가 도달해야 하는 호스트의 포트를 선언합니다. 이는 `[ports]`와 반대 방향입니다 — Coast에서 호스트로 포트를 *내보내는* 대신, egress는 호스트 포트를 Coast *내부에서* 도달 가능하게 만듭니다.

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 48090

[egress]
host-api = 48080
```

이는 Coast 내부의 compose 서비스가 호스트 머신에서 직접 실행 중인 것(코스트의 공유 서비스 시스템 밖)을 대상으로 통신해야 할 때 유용합니다.

규칙:

- `[ports]`와 동일: 값은 0이 아닌 부호 없는 16비트 정수여야 합니다.
- 논리 이름은 자유 형식 식별자입니다.

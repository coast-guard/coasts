# 변수

Coastfile은 모든 문자열 값에서 환경 변수 치환을 지원합니다. 변수는 TOML이 처리되기 전에, 파싱 시점에 해석되므로 어떤 섹션에서든 어떤 값 위치에서든 동작합니다.

## 문법

`${VAR_NAME}`으로 환경 변수를 참조합니다:

```toml
[coast]
name = "${PROJECT_NAME}"
compose = "${COMPOSE_PATH}"

[ports]
web = ${WEB_PORT}
```

변수 이름은 문자 또는 밑줄로 시작해야 하며, 그 뒤에는 문자, 숫자 또는 밑줄이 올 수 있습니다(패턴 `[A-Za-z_][A-Za-z0-9_]*`와 일치).

## 기본값

변수가 설정되지 않았을 때 대체값을 제공하려면 `${VAR:-default}`를 사용합니다:

```toml
[coast]
name = "${PROJECT_NAME:-my-app}"
runtime = "${RUNTIME:-dind}"

[ports]
web = ${WEB_PORT:-3000}
api = ${API_PORT:-8080}
```

환경에서 `PROJECT_NAME`이 설정되어 있으면 그 값이 사용됩니다. 그렇지 않으면 `my-app`이 대체됩니다. 기본값에는 `}`를 제외한 어떤 문자든 포함할 수 있습니다.

## 정의되지 않은 변수

기본값 없이 변수를 참조했고 해당 변수가 환경에 설정되어 있지 않으면, Coast는 **리터럴 `${VAR}` 텍스트를 그대로 보존**하고 경고를 출력합니다:

```
warning: undefined environment variable 'DB_HOST' preserved as literal '${DB_HOST}'; use '${DB_HOST:-}' for explicit empty, or '$${DB_HOST}' to escape entirely
```

참조를 보존하면(빈 문자열로 조용히 대체하는 대신) `ARCH=$(uname -m) && curl .../linux-${ARCH}.tar.gz` 같은 셸 명령이 계속 동작할 수 있습니다 — Coast가 `${ARCH}`를 전혀 설정하지 않았더라도 Dockerfile의 셸은 빌드 시점에 여전히 `${ARCH}`를 확장할 수 있습니다.

변수가 없을 때 실제로 빈 값으로 치환되길 원한다면, 명시적인 빈 기본값을 사용하세요:

```toml
[coast]
name = "${PROJECT_NAME:-}"   # PROJECT_NAME이 설정되지 않았을 때 ""
```

경고 없이 리터럴 `${VAR}` 텍스트를 원한다면 `$${VAR}`로 이스케이프하세요(아래 [이스케이프](#escaping) 참조).

## 이스케이프

Coastfile에서 리터럴 `${...}`를 생성하려면(예를 들어, 값에 확장된 값이 아닌 `${VAR}`라는 텍스트 자체가 들어가야 하는 경우) 앞의 달러 기호를 두 번 씁니다:

```toml
[coast.setup]
run = ["echo '$${NOT_EXPANDED}'"]
```

이렇게 하면 변수 조회를 시도하지 않고 리터럴 문자열 `echo '${NOT_EXPANDED}'`가 생성됩니다.

## 예제

### 환경에서 가져온 키를 사용하는 시크릿

```toml
[secrets.api_key]
extractor = "env"
var = "${API_KEY_ENV_VAR:-MY_API_KEY}"
inject = "env:API_KEY"
```

### 공유 서비스 구성

```toml
[shared_services.postgres]
image = "postgres:${PG_VERSION:-16}"
env = [
    "POSTGRES_USER=${DB_USER:-coast}",
    "POSTGRES_PASSWORD=${DB_PASSWORD:-dev}",
    "POSTGRES_DB=${DB_NAME:-coast_dev}",
]
ports = [5432]
```

### 환경별 compose 경로

```toml
[coast]
name = "my-app"
compose = "${COMPOSE_FILE:-./docker-compose.yml}"
```

## 변수 vs 시크릿

변수 치환과 [시크릿](SECRETS.md)은 서로 다른 목적을 가집니다:

| | 변수 (`${VAR}`) | 시크릿 (`[secrets.*]`) |
|---|---|---|
| **해석 시점** | 파싱 시점(TOML 처리 전) | 빌드 시점(구성된 소스에서 추출) |
| **저장 위치** | 해석된 Coastfile에 내장됨 | 암호화된 키 저장소 (`~/.coast/keystore.db`) |
| **사용 사례** | 환경마다 달라지는 구성(포트, 경로, 이미지 태그) | 민감한 자격 증명(API 키, 토큰, 비밀번호) |
| **아티팩트에 표시 여부** | 예(빌드 내부의 `coastfile.toml`에 값이 나타남) | 아니요(매니페스트에는 시크릿 이름만 나타남) |

머신이나 CI 환경마다 달라지는 비민감 구성에는 변수를 사용하세요. 빌드 아티팩트에 절대 나타나면 안 되는 값에는 시크릿을 사용하세요.

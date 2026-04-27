# 시크릿과 주입

`[secrets.*]` 섹션은 Coast가 빌드 시점에 호스트 머신에서 키체인, 환경 변수, 파일 또는 임의 명령으로부터 자격 증명을 추출하고, 이를 Coast 인스턴스에 환경 변수 또는 파일로 주입하는 방식을 정의합니다. 별도의 `[inject]` 섹션은 추출이나 암호화 없이 비밀이 아닌 호스트 값을 인스턴스로 전달합니다.

시크릿이 런타임에 어떻게 저장되고, 암호화되고, 관리되는지에 대해서는 [Secrets](../concepts_and_terminology/SECRETS.md)를 참조하세요.

시크릿은 [변수 보간](VARIABLES.md)과는 다릅니다. 변수(`${VAR}`)는 파싱 시점에 해석되며 그 값이 빌드 아티팩트에 나타납니다. 시크릿은 빌드 시점에 추출되어 키스토어에 암호화되어 저장되며 -- 그 값은 빌드 아티팩트에 절대 나타나지 않습니다.

## `[secrets.*]`

각 시크릿은 `[secrets]` 아래의 이름 있는 TOML 섹션입니다. 항상 필요한 필드는 두 가지입니다: `extractor`와 `inject`. 추가 필드는 extractor에 매개변수로 전달됩니다.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (필수)

추출 방법의 이름입니다. 내장 extractor:

- **`env`** — 호스트 환경 변수를 읽습니다
- **`file`** — 호스트 파일 시스템에서 파일을 읽습니다
- **`command`** — 셸 명령을 실행하고 stdout을 캡처합니다
- **`keychain`** — macOS 키체인에서 읽습니다(macOS 전용)

커스텀 extractor도 사용할 수 있습니다 — PATH에 `coast-extractor-{name}`이라는 이름의 실행 파일이 있으면 해당 이름의 extractor로 사용할 수 있습니다.

### `inject` (필수)

시크릿 값이 Coast 인스턴스 내부 어디에 배치되는지를 지정합니다. 형식은 두 가지입니다:

- `"env:VAR_NAME"` — 환경 변수로 주입됩니다
- `"file:/absolute/path"` — 파일로 기록됩니다(tmpfs를 통해 마운트됨)

```toml
# 환경 변수로
inject = "env:DATABASE_URL"

# 파일로
inject = "file:/run/secrets/db_password"
```

`env:` 또는 `file:` 뒤의 값은 비어 있으면 안 됩니다.

### `ttl`

선택적 만료 기간입니다. 이 기간이 지나면 시크릿은 오래된 것으로 간주되며, Coast는 다음 빌드에서 extractor를 다시 실행합니다.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### 추가 매개변수

시크릿 섹션의 추가 키(`extractor`, `inject`, `ttl` 제외)는 모두 extractor에 매개변수로 전달됩니다. 어떤 매개변수가 필요한지는 extractor에 따라 다릅니다.

## 내장 extractor

### `env` — 호스트 환경 변수

이름으로 호스트 환경 변수를 읽습니다.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

매개변수: `var` — 읽을 환경 변수 이름.

### `file` — 호스트 파일

호스트 파일 시스템에서 파일의 내용을 읽습니다.

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

매개변수: `path` — 호스트에서의 파일 경로.

### `command` — 셸 명령

호스트에서 셸 명령을 실행하고 stdout을 시크릿 값으로 캡처합니다.

```toml
[secrets.cmd_secret]
extractor = "command"
run = "echo command-secret-value"
inject = "env:CMD_SECRET"
```

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; d=json.load(open(\"$HOME/.claude.json\")); print(json.dumps({k:d[k] for k in [\"oauthAccount\"] if k in d}))"'
inject = "file:/root/.claude.json"
```

매개변수: `run` — 실행할 셸 명령.

### `keychain` — macOS 키체인

macOS 키체인에서 자격 증명을 읽습니다. macOS에서만 사용할 수 있으며, 다른 플랫폼에서 이 extractor를 참조하면 빌드 시점 오류가 발생합니다.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

매개변수: `service` — 조회할 키체인 서비스 이름.

## `[inject]`

`[inject]` 섹션은 시크릿 추출 및 암호화 시스템을 거치지 않고 호스트 환경 변수와 파일을 Coast 인스턴스로 전달합니다. 서비스가 호스트로부터 필요로 하는 비민감 값에 이 기능을 사용하세요.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — 전달할 호스트 환경 변수 이름 목록
- **`files`** — 인스턴스에 마운트할 호스트 파일 경로 목록

## 예제

### 여러 extractor

```toml
[secrets.file_secret]
extractor = "file"
path = "./test-secret.txt"
inject = "env:FILE_SECRET"

[secrets.env_secret]
extractor = "env"
var = "COAST_TEST_ENV_SECRET"
inject = "env:ENV_SECRET"

[secrets.cmd_secret]
extractor = "command"
run = "echo command-secret-value"
inject = "env:CMD_SECRET"

[secrets.file_inject_secret]
extractor = "file"
path = "./test-secret.txt"
inject = "file:/run/secrets/test_secret"
```

### macOS 키체인에서 가져오는 Claude Code 인증

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"

[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; d=json.load(open(\"$HOME/.claude.json\")); out={\"hasCompletedOnboarding\":True,\"numStartups\":1}; print(json.dumps(out))"'
inject = "file:/root/.claude.json"
```

### TTL이 있는 시크릿

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

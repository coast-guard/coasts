# 시크릿과 인젝션

`[secrets.*]` 섹션은 Coast가 빌드 시점에 호스트 머신에서 자격 증명(키체인, 환경 변수, 파일, 또는 임의의 명령)을 추출하고, 이를 Coast 인스턴스에 환경 변수 또는 파일로 주입하도록 정의합니다. 별도의 `[inject]` 섹션은 추출이나 암호화 없이 비-시크릿 호스트 값을 인스턴스로 전달합니다.

시크릿이 런타임에서 어떻게 저장, 암호화, 관리되는지에 대해서는 [Secrets](../concepts_and_terminology/SECRETS.md)를 참고하세요.

## `[secrets.*]`

각 시크릿은 `[secrets]` 아래의 이름이 있는 TOML 섹션입니다. 두 필드는 항상 필요합니다: `extractor`와 `inject`. 추가 필드는 추출기에 대한 매개변수로 전달됩니다.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (필수)

추출 방식의 이름입니다. 내장 추출기:

- **`env`** — 호스트 환경 변수를 읽습니다
- **`file`** — 호스트 파일시스템에서 파일을 읽습니다
- **`command`** — 셸 명령을 실행하고 stdout을 캡처합니다
- **`keychain`** — macOS Keychain에서 읽습니다 (macOS 전용)

커스텀 추출기도 사용할 수 있습니다 — PATH에 있는 실행 파일 중 `coast-extractor-{name}` 형식의 이름을 가진 것은 해당 이름의 추출기로 사용 가능합니다.

### `inject` (필수)

시크릿 값이 Coast 인스턴스 내부에 배치되는 위치입니다. 두 가지 형식:

- `"env:VAR_NAME"` — 환경 변수로 주입됩니다
- `"file:/absolute/path"` — 파일로 기록됩니다 (tmpfs를 통해 마운트됨)

```toml
# 환경 변수로
inject = "env:DATABASE_URL"

# 파일로
inject = "file:/run/secrets/db_password"
```

`env:` 또는 `file:` 뒤의 값은 비어 있으면 안 됩니다.

### `ttl`

선택적 만료 기간입니다. 이 기간이 지나면 시크릿은 오래된 것으로 간주되며, Coast는 다음 빌드에서 추출기를 다시 실행합니다.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### 추가 파라미터

시크릿 섹션에서(`extractor`, `inject`, `ttl`을 제외한) 추가 키는 모두 추출기에 대한 매개변수로 전달됩니다. 어떤 매개변수가 필요한지는 추출기에 따라 다릅니다.

## 내장 추출기

### `env` — 호스트 환경 변수

이름으로 호스트 환경 변수를 읽습니다.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

파라미터: `var` — 읽을 환경 변수 이름.

### `file` — 호스트 파일

호스트 파일시스템에서 파일의 내용을 읽습니다.

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

파라미터: `path` — 호스트에서의 파일 경로.

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

파라미터: `run` — 실행할 셸 명령.

### `keychain` — macOS Keychain

macOS Keychain에서 자격 증명을 읽습니다. macOS에서만 사용 가능하며, 다른 플랫폼에서 이 추출기를 참조하면 빌드 시점 오류가 발생합니다.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

파라미터: `service` — 조회할 Keychain 서비스 이름.

## `[inject]`

`[inject]` 섹션은 시크릿 추출 및 암호화 시스템을 거치지 않고 호스트 환경 변수와 파일을 Coast 인스턴스로 전달합니다. 서비스가 호스트로부터 필요로 하는 비민감 값에 이를 사용하세요.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — 전달할 호스트 환경 변수 이름 목록
- **`files`** — 인스턴스에 마운트할 호스트 파일 경로 목록

## 예시

### 여러 추출기

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

### macOS Keychain에서의 Claude Code 인증

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

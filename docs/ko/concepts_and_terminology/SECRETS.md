# 시크릿과 추출기

시크릿은 호스트 머신에서 추출되어 Coast 컨테이너에 환경 변수 또는 파일로 주입되는 값입니다. Coast는 빌드 시점에 시크릿을 추출하고, 로컬 키스토어에 저장할 때 암호화하며, Coast 인스턴스가 생성될 때 이를 주입합니다.

## 주입 유형

모든 시크릿에는 Coast 컨테이너로 어떻게 전달될지 제어하는 `inject` 대상이 있습니다:

- `env:VAR_NAME` — 환경 변수로 주입됩니다.
- `file:/path/in/container` — 컨테이너 내부에 파일로 마운트됩니다.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.credentials]
extractor = "file"
path = "~/.config/my-app/credentials.json"
inject = "file:/run/secrets/credentials.json"
```

## 내장 추출기

### env

호스트 환경 변수를 읽습니다. 가장 흔하고 가장 단순한 추출기입니다. 호스트에 이미 환경 변수로 시크릿이 있다면 — `.env` 파일, `direnv`, 셸 프로필, 또는 다른 어떤 소스에서든 — 그것들을 Coast로 그대로 전달하기만 하면 됩니다.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DATABASE_PASSWORD"
```

대부분의 프로젝트는 `env` 추출기만으로도 충분합니다.

### file

호스트 파일시스템에서 파일을 읽습니다. 홈 디렉터리 경로에 대해 `~` 확장을 지원합니다. SSH 키, TLS 인증서, 자격 증명 JSON 파일에 적합합니다.

```toml
[secrets.ssh_key]
extractor = "file"
path = "~/.ssh/id_ed25519"
inject = "file:/run/secrets/ssh_key"
```

### command

셸 명령을 실행하고 stdout을 시크릿 값으로 캡처합니다. 명령은 `sh -c`를 통해 실행되므로 파이프, 리다이렉트, 변수 확장이 모두 동작합니다. 이는 1Password CLI, HashiCorp Vault, 또는 어떤 동적 소스에서든 시크릿을 가져오는 데 유용합니다.

```toml
[secrets.op_token]
extractor = "command"
run = "op read 'op://vault/db/password'"
inject = "env:DATABASE_PASSWORD"
```

또한 `command`를 사용해 로컬 설정 파일에서 특정 필드를 변환하거나 추출할 수도 있습니다:

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; print(json.dumps({\"key\": \"value\"}))"'
inject = "file:/root/.claude.json"
```

### keychain

`macos-keychain`의 별칭입니다. macOS 키체인에서 일반 비밀번호 항목을 읽습니다.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

키체인 추출기는 종종 불필요합니다. 동일한 값을 환경 변수나 파일을 통해 얻을 수 있다면, 더 단순한 그 방식들을 우선하세요. 키체인 추출은 시크릿이 macOS 키체인에만 존재하고 쉽게 내보낼 수 없을 때 유용합니다 — 예를 들어 키체인에 직접 기록하는 서드파티 도구가 저장한 애플리케이션 전용 자격 증명 같은 경우입니다.

`account` 매개변수는 선택 사항이며 기본값은 macOS 사용자 이름입니다.

이 추출기는 macOS에서만 사용할 수 있습니다. 다른 플랫폼에서 이를 참조하면 빌드 시점에 명확한 오류가 발생합니다.

## 사용자 정의 추출기

내장 추출기 중 어느 것도 워크플로에 맞지 않으면, Coast는 PATH에서 `coast-extractor-{name}`라는 이름의 실행 파일을 찾는 것으로 폴백합니다. 이 실행 파일은 extractor 매개변수를 stdin으로 JSON 형태로 받고, 시크릿 값을 stdout으로 출력해야 합니다.

```toml
[secrets.vault_token]
extractor = "vault"
path = "secret/data/token"
inject = "env:VAULT_TOKEN"
```

Coast는 stdin으로 `{"path": "secret/data/token"}`를 전달하며 `coast-extractor-vault`를 호출합니다. 종료 코드 0은 성공을 의미하고, 0이 아니면 실패를 의미합니다( stderr는 오류 메시지에 포함됩니다).

## 비-시크릿 주입

`[inject]` 섹션은 호스트 환경 변수와 파일을 시크릿으로 취급하지 않고 Coast로 전달합니다. 이 값들은 암호화되지 않으며 — 직접 전달됩니다.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.gitconfig", "~/.npmrc"]
```

민감하지 않은 구성에는 `[inject]`를 사용하세요. 저장 시 암호화되어야 하는 모든 것은 `[secrets]`를 사용하세요.

## 시크릿은 빌드에 저장되지 않습니다

시크릿은 빌드 시점에 추출되지만 coast 빌드 아티팩트에 포함되어 굽혀지지 않습니다. `coast run`으로 Coast 인스턴스를 생성할 때 주입됩니다. 즉, 시크릿을 노출하지 않고도 빌드 아티팩트를 공유할 수 있습니다.

시크릿은 재빌드 없이 런타임에 다시 주입할 수 있습니다. [Coastguard](COASTGUARD.md) UI에서는 Secrets 탭에서 **Re-run Secrets** 작업을 사용하세요. CLI에서는 [`coast build --refresh`](BUILDS.md)를 사용하여 시크릿을 다시 추출하고 업데이트하세요.

## TTL 및 재추출

시크릿에는 선택적으로 `ttl`(time-to-live) 필드를 가질 수 있습니다. 시크릿이 만료되면 `coast build --refresh`가 소스에서 이를 다시 추출합니다.

```toml
[secrets.short_lived_token]
extractor = "command"
run = "generate-token --ttl 1h"
inject = "env:AUTH_TOKEN"
ttl = "1h"
```

## 저장 시 암호화

추출된 모든 시크릿은 로컬 키스토어에서 AES-256-GCM으로 암호화됩니다. 암호화 키는 macOS에서는 macOS 키체인에 저장되며, Linux에서는 0600 권한의 파일에 저장됩니다.

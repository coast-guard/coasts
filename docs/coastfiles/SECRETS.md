# Secrets and Injection

The `[secrets.*]` sections define credentials that Coast extracts from your host machine at build time — keychains, environment variables, files, or arbitrary commands — and injects into Coast instances as environment variables or files. The separate `[inject]` section forwards non-secret host values into instances without extraction or encryption.

For how secrets are stored, encrypted, and managed at runtime, see [Secrets](../concepts_and_terminology/SECRETS.md).

## `[secrets.*]`

Each secret is a named TOML section under `[secrets]`. Two fields are always required: `extractor` and `inject`. Additional fields are passed as parameters to the extractor.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (required)

The name of the extraction method. Built-in extractors:

- **`env`** — reads a host environment variable
- **`file`** — reads a file from the host filesystem
- **`command`** — runs a shell command and captures stdout
- **`keychain`** — reads from the macOS Keychain (macOS only)

You can also use custom extractors — any executable on your PATH named `coast-extractor-{name}` is available as an extractor by that name.

### `inject` (required)

Where the secret value is placed inside the Coast instance. Two formats:

- `"env:VAR_NAME"` — injected as an environment variable
- `"file:/absolute/path"` — written to a file (mounted via tmpfs)

```toml
# As an environment variable
inject = "env:DATABASE_URL"

# As a file
inject = "file:/run/secrets/db_password"
```

The value after `env:` or `file:` must not be empty.

### `ttl`

Optional expiry duration. After this period, the secret is considered stale and Coast re-runs the extractor on the next build.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### Extra parameters

Any additional keys in a secret section (beyond `extractor`, `inject`, and `ttl`) are passed as parameters to the extractor. Which parameters are needed depends on the extractor.

## Built-in extractors

### `env` — host environment variable

Reads a host environment variable by name.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

Parameter: `var` — the environment variable name to read.

### `file` — host file

Reads the contents of a file from the host filesystem.

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

Parameter: `path` — path to the file on the host.

### `command` — shell command

Runs a shell command on the host and captures stdout as the secret value.

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

Parameter: `run` — the shell command to execute.

### `keychain` — macOS Keychain

Reads a credential from the macOS Keychain. Only available on macOS — referencing this extractor on other platforms produces a build-time error.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

Parameter: `service` — the Keychain service name to look up.

## `[inject]`

The `[inject]` section forwards host environment variables and files into Coast instances without going through the secret extraction and encryption system. Use this for non-sensitive values that your services need from the host.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — list of host environment variable names to forward
- **`files`** — list of host file paths to mount into the instance

## Examples

### Multiple extractors

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

### Claude Code authentication from macOS Keychain

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

### Secrets with TTL

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

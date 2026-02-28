# Secrets and Extractors

Secrets are values extracted from your host machine and injected into Coast containers as environment variables or files. Coast extracts secrets at build time, encrypts them at rest in a local keystore, and injects them when a Coast instance is created.

## Injection Types

Every secret has an `inject` target that controls how it is delivered into the Coast container:

- `env:VAR_NAME` — injected as an environment variable.
- `file:/path/in/container` — mounted as a file inside the container.

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

## Built-in Extractors

### env

Reads a host environment variable. This is the most common and simplest extractor. If you already have secrets as environment variables on your host — from `.env` files, `direnv`, shell profiles, or any other source — just forward them into the Coast.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DATABASE_PASSWORD"
```

Most projects can get by with the `env` extractor alone.

### file

Reads a file from the host filesystem. Supports `~` expansion for home directory paths. Good for SSH keys, TLS certificates, and credential JSON files.

```toml
[secrets.ssh_key]
extractor = "file"
path = "~/.ssh/id_ed25519"
inject = "file:/run/secrets/ssh_key"
```

### command

Runs a shell command and captures stdout as the secret value. The command is executed via `sh -c`, so pipes, redirects, and variable expansion all work. This is useful for pulling secrets from 1Password CLI, HashiCorp Vault, or any dynamic source.

```toml
[secrets.op_token]
extractor = "command"
run = "op read 'op://vault/db/password'"
inject = "env:DATABASE_PASSWORD"
```

You can also use `command` to transform or extract specific fields from local config files:

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; print(json.dumps({\"key\": \"value\"}))"'
inject = "file:/root/.claude.json"
```

### keychain

Alias for `macos-keychain`. Reads a generic password item from the macOS Keychain.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

The keychain extractor is often unnecessary. If you can get the same value via an environment variable or a file, prefer those simpler approaches. Keychain extraction is useful when the secret only exists in the macOS Keychain and is not easily exported — for example, application-specific credentials stored by third-party tools that write directly to the Keychain.

The `account` parameter is optional and defaults to your macOS username.

This extractor is only available on macOS. Referencing it on other platforms produces a clear error at build time.

## Custom Extractors

If none of the built-in extractors fit your workflow, Coast falls back to looking for an executable named `coast-extractor-{name}` on your PATH. The executable receives the extractor parameters as JSON on stdin and should write the secret value to stdout.

```toml
[secrets.vault_token]
extractor = "vault"
path = "secret/data/token"
inject = "env:VAULT_TOKEN"
```

Coast will invoke `coast-extractor-vault`, passing `{"path": "secret/data/token"}` on stdin. Exit code 0 means success; non-zero means failure (stderr is included in the error message).

## Non-secret Injection

The `[inject]` section forwards host environment variables and files into the Coast without treating them as secrets. These values are not encrypted — they are passed directly.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.gitconfig", "~/.npmrc"]
```

Use `[inject]` for configuration that is not sensitive. Use `[secrets]` for anything that should be encrypted at rest.

## Secrets Are Not Stored in the Build

Secrets are extracted at build time but are not baked into the coast build artifact. They are injected when a Coast instance is created with `coast run`. This means you can share build artifacts without exposing secrets.

Secrets can be re-injected at runtime without rebuilding. In the [Coastguard](COASTGUARD.md) UI, use the **Re-run Secrets** action on the Secrets tab. From the CLI, use [`coast build --refresh`](BUILDS.md) to re-extract and update secrets.

## TTL and Re-extraction

Secrets can have an optional `ttl` (time-to-live) field. When a secret expires, `coast build --refresh` will re-extract it from the source.

```toml
[secrets.short_lived_token]
extractor = "command"
run = "generate-token --ttl 1h"
inject = "env:AUTH_TOKEN"
ttl = "1h"
```

## Encryption at Rest

All extracted secrets are encrypted with AES-256-GCM in a local keystore. The encryption key is stored in the macOS Keychain on macOS, or in a file with 0600 permissions on Linux.

# Variables

Coastfiles support environment variable interpolation in all string values. Variables are resolved at parse time, before the TOML is processed, so they work in any section and any value position.

## Syntax

Reference an environment variable with `${VAR_NAME}`:

```toml
[coast]
name = "${PROJECT_NAME}"
compose = "${COMPOSE_PATH}"

[ports]
web = ${WEB_PORT}
```

Variable names must start with a letter or underscore, followed by letters, digits, or underscores (matching the pattern `[A-Za-z_][A-Za-z0-9_]*`).

## Default Values

Use `${VAR:-default}` to provide a fallback when the variable is not set:

```toml
[coast]
name = "${PROJECT_NAME:-my-app}"
runtime = "${RUNTIME:-dind}"

[ports]
web = ${WEB_PORT:-3000}
api = ${API_PORT:-8080}
```

If `PROJECT_NAME` is set in the environment, its value is used. If not, `my-app` is substituted. Default values can contain any characters except `}`.

## Undefined Variables

When a variable is referenced without a default and is not set in the environment, Coast **preserves the literal `${VAR}` text** and emits a warning:

```
warning: undefined environment variable 'DB_HOST' preserved as literal '${DB_HOST}'; use '${DB_HOST:-}' for explicit empty, or '$${DB_HOST}' to escape entirely
```

Preserving the reference (instead of silently replacing it with an empty string) keeps shell commands like `ARCH=$(uname -m) && curl .../linux-${ARCH}.tar.gz` working — the Dockerfile's shell can still expand `${ARCH}` at build time even though Coast never set it.

If you actually want an empty substitution when the variable is missing, use the explicit empty default:

```toml
[coast]
name = "${PROJECT_NAME:-}"   # "" when PROJECT_NAME is unset
```

If you want the literal `${VAR}` text without any warning, escape it with `$${VAR}` (see [Escaping](#escaping) below).

## Escaping

To produce a literal `${...}` in your Coastfile (for example, in a value that should contain the text `${VAR}` rather than its expanded value), double the leading dollar sign:

```toml
[coast.setup]
run = ["echo '$${NOT_EXPANDED}'"]
```

This produces the literal string `echo '${NOT_EXPANDED}'` without attempting variable lookup.

## Examples

### Secrets with environment-sourced keys

```toml
[secrets.api_key]
extractor = "env"
var = "${API_KEY_ENV_VAR:-MY_API_KEY}"
inject = "env:API_KEY"
```

### Shared service configuration

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

### Per-environment compose path

```toml
[coast]
name = "my-app"
compose = "${COMPOSE_FILE:-./docker-compose.yml}"
```

## Variables vs Secrets

Variable interpolation and [secrets](SECRETS.md) serve different purposes:

| | Variables (`${VAR}`) | Secrets (`[secrets.*]`) |
|---|---|---|
| **When resolved** | Parse time (before TOML processing) | Build time (extracted from configured sources) |
| **Where stored** | Baked into the resolved Coastfile | Encrypted keystore (`~/.coast/keystore.db`) |
| **Use case** | Configuration that varies per environment (ports, paths, image tags) | Sensitive credentials (API keys, tokens, passwords) |
| **Visible in artifacts** | Yes (values appear in `coastfile.toml` inside the build) | No (only secret names appear in manifest) |

Use variables for non-sensitive configuration that changes between machines or CI environments. Use secrets for values that should never appear in build artifacts.

# Secretos e inyección

Las secciones `[secrets.*]` definen credenciales que Coast extrae de tu máquina host en tiempo de build — llaveros, variables de entorno, archivos o comandos arbitrarios — e inyecta en instancias de Coast como variables de entorno o archivos. La sección separada `[inject]` reenvía valores no secretos del host a las instancias sin extracción ni cifrado.

Para saber cómo se almacenan, cifran y gestionan los secretos en tiempo de ejecución, consulta [Secrets](../concepts_and_terminology/SECRETS.md).

## `[secrets.*]`

Cada secreto es una sección TOML con nombre bajo `[secrets]`. Siempre se requieren dos campos: `extractor` e `inject`. Los campos adicionales se pasan como parámetros al extractor.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (obligatorio)

El nombre del método de extracción. Extractores integrados:

- **`env`** — lee una variable de entorno del host
- **`file`** — lee un archivo del sistema de archivos del host
- **`command`** — ejecuta un comando de shell y captura stdout
- **`keychain`** — lee del Llavero de macOS (solo macOS)

También puedes usar extractores personalizados: cualquier ejecutable en tu PATH llamado `coast-extractor-{name}` está disponible como un extractor con ese nombre.

### `inject` (obligatorio)

Dónde se coloca el valor del secreto dentro de la instancia de Coast. Dos formatos:

- `"env:VAR_NAME"` — se inyecta como una variable de entorno
- `"file:/absolute/path"` — se escribe en un archivo (montado vía tmpfs)

```toml
# Como una variable de entorno
inject = "env:DATABASE_URL"

# Como un archivo
inject = "file:/run/secrets/db_password"
```

El valor después de `env:` o `file:` no debe estar vacío.

### `ttl`

Duración de caducidad opcional. Después de este período, el secreto se considera obsoleto y Coast vuelve a ejecutar el extractor en el próximo build.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### Parámetros adicionales

Cualquier clave adicional en una sección de secreto (más allá de `extractor`, `inject` y `ttl`) se pasa como parámetro al extractor. Qué parámetros se necesitan depende del extractor.

## Extractores integrados

### `env` — variable de entorno del host

Lee una variable de entorno del host por nombre.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

Parámetro: `var` — el nombre de la variable de entorno a leer.

### `file` — archivo del host

Lee el contenido de un archivo del sistema de archivos del host.

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

Parámetro: `path` — la ruta al archivo en el host.

### `command` — comando de shell

Ejecuta un comando de shell en el host y captura stdout como el valor del secreto.

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

Parámetro: `run` — el comando de shell a ejecutar.

### `keychain` — Llavero de macOS

Lee una credencial del Llavero de macOS. Solo está disponible en macOS; referenciar este extractor en otras plataformas produce un error en tiempo de build.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

Parámetro: `service` — el nombre del servicio del Llavero que se debe buscar.

## `[inject]`

La sección `[inject]` reenvía variables de entorno y archivos del host a instancias de Coast sin pasar por el sistema de extracción y cifrado de secretos. Usa esto para valores no sensibles que tus servicios necesiten del host.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — lista de nombres de variables de entorno del host que se reenviarán
- **`files`** — lista de rutas de archivos del host que se montarán en la instancia

## Ejemplos

### Múltiples extractores

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

### Autenticación de Claude Code desde el Llavero de macOS

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

### Secretos con TTL

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

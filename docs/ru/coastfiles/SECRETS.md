# Секреты и инъекция

Разделы `[secrets.*]` определяют учётные данные, которые Coast извлекает с вашей хост-машины во время сборки — связки ключей, переменные окружения, файлы или произвольные команды — и внедряет в экземпляры Coast как переменные окружения или файлы. Отдельный раздел `[inject]` пробрасывает в экземпляры несекретные значения с хоста без извлечения или шифрования.

О том, как секреты хранятся, шифруются и управляются во время выполнения, см. [Secrets](../concepts_and_terminology/SECRETS.md).

## `[secrets.*]`

Каждый секрет — это именованный TOML-раздел внутри `[secrets]`. Два поля всегда обязательны: `extractor` и `inject`. Дополнительные поля передаются как параметры экстрактору.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (обязательно)

Имя метода извлечения. Встроенные экстракторы:

- **`env`** — читает переменную окружения хоста
- **`file`** — читает файл из файловой системы хоста
- **`command`** — запускает команду оболочки и захватывает stdout
- **`keychain`** — читает из macOS Keychain (только macOS)

Также можно использовать пользовательские экстракторы — любой исполняемый файл в вашем PATH с именем `coast-extractor-{name}` доступен как экстрактор с этим именем.

### `inject` (обязательно)

Куда значение секрета помещается внутри экземпляра Coast. Два формата:

- `"env:VAR_NAME"` — внедряется как переменная окружения
- `"file:/absolute/path"` — записывается в файл (монтируется через tmpfs)

```toml
# Как переменная окружения
inject = "env:DATABASE_URL"

# Как файл
inject = "file:/run/secrets/db_password"
```

Значение после `env:` или `file:` не должно быть пустым.

### `ttl`

Необязательная длительность до истечения срока. По истечении этого периода секрет считается устаревшим, и Coast повторно запускает экстрактор при следующей сборке.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### Дополнительные параметры

Любые дополнительные ключи в разделе секрета (кроме `extractor`, `inject` и `ttl`) передаются как параметры экстрактору. Какие параметры нужны, зависит от экстрактора.

## Встроенные экстракторы

### `env` — переменная окружения хоста

Читает переменную окружения хоста по имени.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

Параметр: `var` — имя переменной окружения для чтения.

### `file` — файл хоста

Читает содержимое файла из файловой системы хоста.

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

Параметр: `path` — путь к файлу на хосте.

### `command` — команда оболочки

Запускает команду оболочки на хосте и захватывает stdout как значение секрета.

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

Параметр: `run` — команда оболочки для выполнения.

### `keychain` — macOS Keychain

Читает учётные данные из macOS Keychain. Доступно только на macOS — ссылка на этот экстрактор на других платформах приводит к ошибке на этапе сборки.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

Параметр: `service` — имя сервиса Keychain для поиска.

## `[inject]`

Раздел `[inject]` пробрасывает переменные окружения и файлы хоста в экземпляры Coast без прохождения через систему извлечения и шифрования секретов. Используйте это для нечувствительных значений, которые вашим сервисам нужны с хоста.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — список имён переменных окружения хоста для проброса
- **`files`** — список путей к файлам на хосте для монтирования в экземпляр

## Примеры

### Несколько экстракторов

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

### Аутентификация Claude Code из macOS Keychain

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

### Секреты с TTL

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

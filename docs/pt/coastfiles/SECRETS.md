# Segredos e Injeção

As seções `[secrets.*]` definem credenciais que o Coast extrai da sua máquina host no momento do build — chaveiros, variáveis de ambiente, arquivos ou comandos arbitrários — e injeta em instâncias do Coast como variáveis de ambiente ou arquivos. A seção separada `[inject]` encaminha valores não secretos do host para as instâncias sem extração ou criptografia.

Para saber como os segredos são armazenados, criptografados e gerenciados em tempo de execução, veja [Secrets](../concepts_and_terminology/SECRETS.md).

## `[secrets.*]`

Cada segredo é uma seção TOML nomeada sob `[secrets]`. Dois campos são sempre obrigatórios: `extractor` e `inject`. Campos adicionais são passados como parâmetros para o extractor.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
```

### `extractor` (obrigatório)

O nome do método de extração. Extractors embutidos:

- **`env`** — lê uma variável de ambiente do host
- **`file`** — lê um arquivo do sistema de arquivos do host
- **`command`** — executa um comando de shell e captura o stdout
- **`keychain`** — lê do Keychain do macOS (apenas macOS)

Você também pode usar extractors personalizados — qualquer executável no seu PATH chamado `coast-extractor-{name}` fica disponível como um extractor com esse nome.

### `inject` (obrigatório)

Onde o valor do segredo é colocado dentro da instância do Coast. Dois formatos:

- `"env:VAR_NAME"` — injetado como uma variável de ambiente
- `"file:/absolute/path"` — gravado em um arquivo (montado via tmpfs)

```toml
# As an environment variable
inject = "env:DATABASE_URL"

# As a file
inject = "file:/run/secrets/db_password"
```

O valor após `env:` ou `file:` não deve estar vazio.

### `ttl`

Duração de expiração opcional. Após esse período, o segredo é considerado desatualizado e o Coast reexecuta o extractor no próximo build.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
ttl = "1h"
```

### Parâmetros extras

Quaisquer chaves adicionais em uma seção de segredo (além de `extractor`, `inject` e `ttl`) são passadas como parâmetros para o extractor. Quais parâmetros são necessários depende do extractor.

## Extractors embutidos

### `env` — variável de ambiente do host

Lê uma variável de ambiente do host pelo nome.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"
```

Parâmetro: `var` — o nome da variável de ambiente a ser lida.

### `file` — arquivo do host

Lê o conteúdo de um arquivo do sistema de arquivos do host.

```toml
[secrets.tls_cert]
extractor = "file"
path = "./certs/dev.pem"
inject = "file:/etc/ssl/certs/dev.pem"
```

Parâmetro: `path` — caminho para o arquivo no host.

### `command` — comando de shell

Executa um comando de shell no host e captura o stdout como o valor do segredo.

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

Parâmetro: `run` — o comando de shell a executar.

### `keychain` — Keychain do macOS

Lê uma credencial do Keychain do macOS. Disponível apenas no macOS — referenciar esse extractor em outras plataformas produz um erro em tempo de build.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

Parâmetro: `service` — o nome do serviço no Keychain a ser procurado.

## `[inject]`

A seção `[inject]` encaminha variáveis de ambiente e arquivos do host para instâncias do Coast sem passar pelo sistema de extração e criptografia de segredos. Use isto para valores não sensíveis que seus serviços precisam obter do host.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.npmrc", "~/.gitconfig"]
```

- **`env`** — lista de nomes de variáveis de ambiente do host a encaminhar
- **`files`** — lista de caminhos de arquivos do host a montar na instância

## Exemplos

### Vários extractors

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

### Autenticação do Claude Code a partir do Keychain do macOS

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

### Segredos com TTL

```toml
[secrets.short_lived_token]
extractor = "command"
run = "vault read -field=token secret/myapp"
inject = "env:VAULT_TOKEN"
ttl = "30m"
```

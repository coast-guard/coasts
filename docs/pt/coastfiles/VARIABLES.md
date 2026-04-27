# Variáveis

Os Coastfiles oferecem suporte à interpolação de variáveis de ambiente em todos os valores de string. As variáveis são resolvidas no momento da análise, antes de o TOML ser processado, portanto funcionam em qualquer seção e em qualquer posição de valor.

## Sintaxe

Referencie uma variável de ambiente com `${VAR_NAME}`:

```toml
[coast]
name = "${PROJECT_NAME}"
compose = "${COMPOSE_PATH}"

[ports]
web = ${WEB_PORT}
```

Os nomes das variáveis devem começar com uma letra ou sublinhado, seguidos por letras, dígitos ou sublinhados (correspondendo ao padrão `[A-Za-z_][A-Za-z0-9_]*`).

## Valores Padrão

Use `${VAR:-default}` para fornecer um valor de fallback quando a variável não estiver definida:

```toml
[coast]
name = "${PROJECT_NAME:-my-app}"
runtime = "${RUNTIME:-dind}"

[ports]
web = ${WEB_PORT:-3000}
api = ${API_PORT:-8080}
```

Se `PROJECT_NAME` estiver definida no ambiente, seu valor será usado. Caso contrário, `my-app` será substituído. Os valores padrão podem conter quaisquer caracteres, exceto `}`.

## Variáveis Não Definidas

Quando uma variável é referenciada sem um valor padrão e não está definida no ambiente, o Coast **preserva o texto literal `${VAR}`** e emite um aviso:

```
warning: undefined environment variable 'DB_HOST' preserved as literal '${DB_HOST}'; use '${DB_HOST:-}' for explicit empty, or '$${DB_HOST}' to escape entirely
```

Preservar a referência (em vez de substituí-la silenciosamente por uma string vazia) mantém comandos de shell como `ARCH=$(uname -m) && curl .../linux-${ARCH}.tar.gz` funcionando — o shell do Dockerfile ainda pode expandir `${ARCH}` no momento do build, mesmo que o Coast nunca a tenha definido.

Se você realmente quiser uma substituição vazia quando a variável estiver ausente, use o valor padrão vazio explícito:

```toml
[coast]
name = "${PROJECT_NAME:-}"   # "" quando PROJECT_NAME não estiver definida
```

Se você quiser o texto literal `${VAR}` sem qualquer aviso, escape-o com `$${VAR}` (veja [Escapando](#escaping) abaixo).

## Escapando

Para produzir um `${...}` literal no seu Coastfile (por exemplo, em um valor que deve conter o texto `${VAR}` em vez de seu valor expandido), duplique o cifrão inicial:

```toml
[coast.setup]
run = ["echo '$${NOT_EXPANDED}'"]
```

Isso produz a string literal `echo '${NOT_EXPANDED}'` sem tentar procurar a variável.

## Exemplos

### Segredos com chaves obtidas do ambiente

```toml
[secrets.api_key]
extractor = "env"
var = "${API_KEY_ENV_VAR:-MY_API_KEY}"
inject = "env:API_KEY"
```

### Configuração de serviço compartilhado

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

### Caminho do compose por ambiente

```toml
[coast]
name = "my-app"
compose = "${COMPOSE_FILE:-./docker-compose.yml}"
```

## Variáveis vs Segredos

A interpolação de variáveis e os [segredos](SECRETS.md) servem a propósitos diferentes:

| | Variáveis (`${VAR}`) | Segredos (`[secrets.*]`) |
|---|---|---|
| **Quando resolvidos** | Momento da análise (antes do processamento do TOML) | Momento do build (extraídos das fontes configuradas) |
| **Onde armazenados** | Incorporados ao Coastfile resolvido | Keystore criptografado (`~/.coast/keystore.db`) |
| **Caso de uso** | Configuração que varia por ambiente (portas, caminhos, tags de imagem) | Credenciais sensíveis (chaves de API, tokens, senhas) |
| **Visível em artefatos** | Sim (os valores aparecem em `coastfile.toml` dentro do build) | Não (apenas os nomes dos segredos aparecem no manifesto) |

Use variáveis para configuração não sensível que muda entre máquinas ou ambientes de CI. Use segredos para valores que nunca devem aparecer em artefatos de build.

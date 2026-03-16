# Projeto e Configuração

A seção `[coast]` é a única seção obrigatória em um Coastfile. Ela identifica o projeto e configura como o contêiner Coast é criado. A subseção opcional `[coast.setup]` permite instalar pacotes e executar comandos dentro do contêiner em tempo de build.

## `[coast]`

### `name` (obrigatório)

Um identificador único para o projeto. Usado em nomes de contêineres, nomes de volumes, rastreamento de estado e saída da CLI.

```toml
[coast]
name = "my-app"
```

### `compose`

Caminho para um arquivo Docker Compose. Caminhos relativos são resolvidos em relação à raiz do projeto (o diretório que contém o Coastfile, ou `root` se definido).

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
```

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
```

Se omitido, o contêiner Coast inicia sem executar `docker compose up`. Você pode usar [serviços bare](SERVICES.md) ou interagir com o contêiner diretamente via `coast exec`.

Você não pode definir ambos `compose` e `[services]` no mesmo Coastfile.

### `runtime`

Qual runtime de contêiner usar. O padrão é `"dind"` (Docker-in-Docker).

- `"dind"` — Docker-in-Docker com `--privileged`. O único runtime testado em produção. Veja [Runtimes and Services](../concepts_and_terminology/RUNTIMES_AND_SERVICES.md).
- `"sysbox"` — Usa o runtime Sysbox em vez do modo privilegiado. Requer o Sysbox instalado.
- `"podman"` — Usa o Podman como o runtime interno de contêiner.

```toml
[coast]
name = "my-app"
runtime = "dind"
```

### `root`

Substitui o diretório raiz do projeto. Por padrão, a raiz do projeto é o diretório que contém o Coastfile. Um caminho relativo é resolvido em relação ao diretório do Coastfile; um caminho absoluto é usado como está.

```toml
[coast]
name = "my-app"
root = "../my-project"
```

Isso é incomum. A maioria dos projetos mantém o Coastfile na raiz real do projeto.

### `worktree_dir`

Diretórios onde worktrees do git ficam. Aceita uma única string ou um array de strings. O padrão é `".worktrees"`.

```toml
# Single directory
worktree_dir = ".worktrees"

# Multiple directories, including an external one
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
```

Caminhos relativos são resolvidos em relação à raiz do projeto. Caminhos que começam com `~/` ou `/` são tratados como diretórios **externos** — o Coast adiciona um bind mount separado para que o contêiner possa acessá-los. É assim que você integra com ferramentas como o Codex que criam worktrees fora da raiz do projeto.

Em tempo de execução, o Coast detecta automaticamente o diretório de worktree a partir de worktrees existentes do git (via `git worktree list`) e prefere isso ao padrão configurado quando todos os worktrees concordam em um único diretório.

Veja [Worktree Directories](WORKTREE_DIR.md) para a referência completa, incluindo comportamento de diretórios externos, filtragem por projeto e exemplos.

### `default_worktree_dir`

Qual diretório usar ao criar **novos** worktrees. O padrão é a primeira entrada em `worktree_dir`. Relevante apenas quando `worktree_dir` é um array.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
default_worktree_dir = ".worktrees"
```

### `autostart`

Se deve executar automaticamente `docker compose up` (ou iniciar serviços bare) quando uma instância do Coast é criada com `coast run`. O padrão é `true`.

Defina como `false` quando você quiser o contêiner em execução, mas quiser iniciar serviços manualmente — útil para variantes de test-runner nas quais você invoca testes sob demanda.

```toml
[coast]
name = "my-app"
extends = "Coastfile"
autostart = false
```

### `primary_port`

Nomeia uma porta da seção `[ports]` para usar em quick-links e roteamento por subdomínio. O valor deve corresponder a uma chave definida em `[ports]`.

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

Veja [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) para saber como isso habilita roteamento por subdomínio e templates de URL.

## `[coast.setup]`

Personaliza o próprio contêiner Coast — instalando ferramentas, executando etapas de build e materializando arquivos de configuração. Tudo em `[coast.setup]` é executado dentro do contêiner DinD (não dentro dos seus serviços do compose).

### `packages`

Pacotes APK para instalar. Estes são pacotes do Alpine Linux, já que a imagem base DinD é baseada em Alpine.

```toml
[coast.setup]
packages = ["nodejs", "npm", "git", "curl"]
```

### `run`

Comandos de shell executados em ordem durante o build. Use-os para instalar ferramentas que não estão disponíveis como pacotes APK.

```toml
[coast.setup]
packages = ["nodejs", "npm", "python3", "wget", "bash", "ca-certificates"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]
```

### `[[coast.setup.files]]`

Arquivos a criar dentro do contêiner. Cada entrada tem um `path` (obrigatório, deve ser absoluto), `content` (obrigatório) e `mode` opcional (string octal de 3-4 dígitos).

```toml
[coast.setup]
packages = ["nodejs", "npm"]
run = ["mkdir -p /app/config"]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

Regras de validação para entradas de arquivo:

- `path` deve ser absoluto (começar com `/`)
- `path` não deve conter componentes `..`
- `path` não deve terminar com `/`
- `mode` deve ser uma string octal de 3 ou 4 dígitos (por exemplo, `"600"`, `"0644"`)

## Exemplo completo

Um contêiner Coast configurado para desenvolvimento em Go e Node.js:

```toml
[coast]
name = "my-fullstack-app"
compose = "./docker-compose.yml"
runtime = "dind"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "make", "curl", "git", "bash", "ca-certificates", "wget", "gcc", "musl-dev"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz && ln -s /usr/local/go/bin/go /usr/local/bin/go",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
    "pip3 install --break-system-packages pgcli",
]

[[coast.setup.files]]
path = "/app/config/dev.json"
content = '''
{
  "logLevel": "debug",
  "featureFlags": { "newDashboard": true }
}
'''
mode = "0644"
```

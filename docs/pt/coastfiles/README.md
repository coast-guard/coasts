# Coastfiles

Um Coastfile é um arquivo de configuração TOML que fica na raiz do seu projeto. Ele informa ao Coast tudo o que ele precisa saber para construir e executar ambientes de desenvolvimento isolados para esse projeto — quais serviços executar, quais portas encaminhar, como lidar com dados e como gerenciar segredos.

Todo projeto Coast precisa de pelo menos um Coastfile. O arquivo sempre se chama `Coastfile` (C maiúsculo, sem extensão). Se você precisar de variantes para diferentes fluxos de trabalho, você cria Coastfiles tipados como `Coastfile.light` ou `Coastfile.snap` que [herdam do base](INHERITANCE.md).

Para um entendimento mais profundo de como os Coastfiles se relacionam com o restante do Coast, veja [Coasts](../concepts_and_terminology/COASTS.md) e [Builds](../concepts_and_terminology/BUILDS.md).

## Quickstart

O menor Coastfile possível:

```toml
[coast]
name = "my-app"
```

Isso oferece a você um container DinD no qual você pode entrar com `coast exec`. A maioria dos projetos vai querer uma referência a `compose` ou [serviços bare](SERVICES.md):

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
```

Ou sem compose, usando serviços bare:

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[ports]
web = 3000
```

Execute `coast build` e depois `coast run dev-1` e você terá um ambiente isolado.

## Example Coastfiles

### Simple bare-service project

Um app Next.js sem arquivo compose. O Coast instala Node, executa `npm install` e inicia o servidor de desenvolvimento diretamente.

```toml
[coast]
name = "my-crm"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Full-stack compose project

Um projeto multi-serviço com bancos de dados compartilhados, segredos, estratégias de volumes e setup personalizado.

```toml
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = ".worktrees"
primary_port = "web"

[coast.setup]
packages = ["nodejs", "npm", "python3", "curl", "git", "bash", "ca-certificates", "wget"]
run = [
    "ARCH=$(uname -m | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/') && wget -qO /tmp/go.tar.gz https://go.dev/dl/go1.24.1.linux-${ARCH}.tar.gz && tar -C /usr/local -xzf /tmp/go.tar.gz && rm /tmp/go.tar.gz",
    "GOBIN=/usr/local/bin go install github.com/air-verse/air@v1.61.7",
]

[ports]
web = 3000
backend = 8080
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:15"
ports = [5432]
volumes = ["infra_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "myapp", POSTGRES_PASSWORD = "myapp_pass" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[volumes.go_modules_cache]
strategy = "shared"
service = "backend"
mount = "/go/pkg/mod"

[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DB_PASSWORD"

[omit]
services = ["monitoring", "admin-panel", "nginx-proxy"]

[assign]
default = "none"
[assign.services]
backend = "hot"
web = "hot"
```

### Lightweight test variant (inheritance)

Estende o Coastfile base, mas o reduz apenas ao necessário para executar testes de backend. Sem portas, sem serviços compartilhados, bancos de dados isolados.

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "backend", "postgres", "redis"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
backend-test = "rebuild"
```

### Snapshot-seeded variant

Cada instância do coast inicia com uma cópia dos volumes de banco de dados existentes no host e, em seguida, diverge de forma independente.

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis", "mongodb"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "infra_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "infra_redis_data"
service = "redis"
mount = "/data"

[volumes.mongodb_data]
strategy = "isolated"
snapshot_source = "infra_mongodb_data"
service = "mongodb"
mount = "/data/db"
```

## Conventions

- O arquivo deve se chamar `Coastfile` (C maiúsculo, sem extensão) e ficar na raiz do projeto.
- Variantes tipadas usam o padrão `Coastfile.{type}` — por exemplo `Coastfile.light`, `Coastfile.snap`. Veja [Inheritance and Types](INHERITANCE.md).
- O nome reservado `Coastfile.default` não é permitido.
- A sintaxe TOML é usada em todo lugar. Todos os cabeçalhos de seção usam `[colchetes]` e entradas nomeadas usam `[section.name]` (não array-of-tables).
- Você não pode usar `compose` e `[services]` no mesmo Coastfile — escolha um.
- Caminhos relativos (para `compose`, `root`, etc.) são resolvidos em relação ao diretório pai do Coastfile.

## Reference

| Page | Sections | What it covers |
|------|----------|----------------|
| [Project and Setup](PROJECT.md) | `[coast]`, `[coast.setup]` | Nome, caminho do compose, runtime, diretório de worktree, setup do container |
| [Ports](PORTS.md) | `[ports]`, `[egress]` | Encaminhamento de portas, declarações de egress, porta primária |
| [Volumes](VOLUMES.md) | `[volumes.*]` | Estratégias de volumes isolados, compartilhados e semeados por snapshot |
| [Shared Services](SHARED_SERVICES.md) | `[shared_services.*]` | Bancos de dados em nível de host e serviços de infraestrutura |
| [Secrets](SECRETS.md) | `[secrets.*]`, `[inject]` | Extração e injeção de segredos e encaminhamento de env/arquivo do host |
| [Bare Services](SERVICES.md) | `[services.*]` | Executar processos diretamente sem Docker Compose |
| [Agent Shell](AGENT_SHELL.md) | `[agent_shell]` | Runtimes de TUI do agente containerizado |
| [MCP Servers](MCP.md) | `[mcp.*]`, `[mcp_clients.*]` | Servidores MCP internos e proxied a partir do host, conectores de cliente |
| [Assign](ASSIGN.md) | `[assign]` | Comportamento de troca de branch por serviço |
| [Inheritance and Types](INHERITANCE.md) | `extends`, `includes`, `[unset]`, `[omit]` | Coastfiles tipados, composição e substituições |

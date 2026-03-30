# Aplicação Next.js

Esta receita é para uma aplicação Next.js com suporte de Postgres e Redis, com workers em segundo plano ou serviços auxiliares opcionais. A stack executa o Next.js como um [serviço bare](../concepts_and_terminology/BARE_SERVICES.md) com Turbopack para HMR rápido, enquanto Postgres e Redis são executados como [serviços compartilhados](../concepts_and_terminology/SHARED_SERVICES.md) no host para que cada instância do Coast compartilhe os mesmos dados.

Este padrão funciona bem quando:

- Seu projeto usa Next.js com Turbopack em desenvolvimento
- Você tem uma camada de banco de dados e cache (Postgres, Redis) sustentando a aplicação
- Você quer várias instâncias do Coast rodando em paralelo sem configuração de banco de dados por instância
- Você usa bibliotecas de autenticação como NextAuth que incorporam URLs de callback nas respostas

## The Complete Coastfile

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]

[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]

# --- Bare services: Next.js and background worker ---

[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]

[services.worker]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev:worker"
restart = "on-failure"
cache = ["node_modules"]

# --- Shared services: Postgres and Redis on the host ---

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]

# --- Secrets: connection strings for bare services ---

[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"

# --- Ports ---

[ports]
web = 3000
postgres = 5432
redis = 6379

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

## Projeto e Configuração

```toml
[coast]
name = "my-nextjs-app"
primary_port = "web"
private_paths = ["packages/web/.next"]
worktree_dir = [".worktrees", ".claude/worktrees"]
```

**`private_paths`** é crítico para Next.js. O Turbopack cria um arquivo de lock em `.next/dev/lock` na inicialização. Sem `private_paths`, uma segunda instância do Coast no mesmo branch vê o lock e se recusa a iniciar. Com isso, cada instância recebe seu próprio diretório `.next` isolado por meio de uma montagem overlay por instância. Veja [Private Paths](../concepts_and_terminology/PRIVATE_PATHS.md).

**`worktree_dir`** lista diretórios onde ficam os git worktrees. Se você usa múltiplos agentes de programação (Claude Code, Cursor, Codex), cada um pode criar worktrees em locais diferentes. Listar todos eles permite que o Coast descubra e atribua worktrees independentemente de qual ferramenta os criou.

```toml
[coast.setup]
packages = ["nodejs", "npm", "make", "git", "bash"]
run = [
    "npm install -g corepack",
    "corepack enable",
]
```

A seção de setup instala pacotes de sistema e ferramentas necessárias para serviços bare. `corepack enable` ativa yarn ou pnpm com base no campo `packageManager` do projeto. Eles são executados em tempo de build dentro da imagem do Coast, não na inicialização da instância.

## Serviços Bare

```toml
[services.web]
install = [
    "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)",
    "cd /workspace && test -f config.json || echo {} > config.json",
    "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn prisma migrate dev",
]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres REDIS_URL=redis://host.docker.internal:6379 yarn dev"
port = 3000
restart = "on-failure"
cache = ["node_modules"]
```

**Instalações condicionais:** O padrão `test -f node_modules/.yarn-state.yml || make yarn` pula a instalação de dependências se `node_modules` já existir. Isso torna as trocas de branch rápidas quando as dependências não mudaram. Veja [Bare Service Optimization](../concepts_and_terminology/BARE_SERVICE_OPTIMIZATION.md).

**`cache`:** Preserva `node_modules` entre trocas de worktree para que `yarn install` rode incrementalmente em vez de do zero.

**`AUTH_URL` com porta dinâmica:** Aplicações Next.js que usam NextAuth (ou bibliotecas de autenticação semelhantes) incorporam URLs de callback nas respostas. Dentro do Coast, o Next.js escuta na porta 3000, mas a porta do lado do host é dinâmica. O Coast injeta `WEB_DYNAMIC_PORT` no ambiente do container automaticamente (derivado da chave `web` em `[ports]`). O fallback `:-3000` significa que o mesmo comando funciona fora do Coast. Veja [Dynamic Port Environment Variables](../concepts_and_terminology/DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md).

**`host.docker.internal`:** Serviços bare não conseguem alcançar serviços compartilhados via `localhost` porque os serviços compartilhados são executados no daemon Docker do host. `host.docker.internal` resolve para o host de dentro do container do Coast.

## Serviços Compartilhados

```toml
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["myapp_postgres_data:/var/lib/postgresql/data"]
env = { POSTGRES_DB = "postgres", POSTGRES_USER = "postgres", POSTGRES_PASSWORD = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
volumes = ["myapp_redis_data:/data"]
```

Postgres e Redis são executados no daemon Docker do host como [serviços compartilhados](../concepts_and_terminology/SHARED_SERVICES.md). Cada instância do Coast se conecta aos mesmos bancos de dados, então usuários, sessões e dados são compartilhados entre instâncias. Isso evita o problema de precisar se cadastrar separadamente em cada instância.

Se seu projeto já tem um `docker-compose.yml` com Postgres e Redis, você pode usar `compose` no lugar e definir a estratégia de volume como `shared`. Serviços compartilhados são mais simples para Coastfiles com serviços bare porque não há arquivo compose para gerenciar.

## Segredos

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"

[secrets.redis_url]
extractor = "command"
run = "echo redis://host.docker.internal:6379"
inject = "env:REDIS_URL"
```

Eles injetam `DATABASE_URL` e `REDIS_URL` no ambiente do container do Coast em tempo de build. As strings de conexão apontam para os serviços compartilhados via `host.docker.internal`.

O extractor `command` executa um comando de shell e captura stdout. Aqui ele apenas faz echo de uma string estática, mas você pode usá-lo para ler de um vault, executar uma ferramenta de CLI ou calcular um valor dinamicamente.

Observe que os campos `command` dos serviços bare também definem essas variáveis inline. Os valores inline têm precedência, mas os segredos injetados servem como padrão para etapas `install` e sessões `coast exec`.

## Estratégias de Assign

```toml
[assign]
default = "none"
exclude_paths = ["docs", ".github", "scripts"]

[assign.services]
web = "hot"
worker = "hot"

[assign.rebuild_triggers]
web = ["package.json", "yarn.lock"]
worker = ["package.json", "yarn.lock"]
```

**`default = "none"`** deixa serviços compartilhados e infraestrutura intactos na troca de branch. Apenas os serviços que dependem de código recebem uma estratégia de assign.

**`hot` para Next.js e workers:** Next.js com Turbopack tem hot module replacement embutido. Quando o Coast remonta `/workspace` para o novo worktree, o Turbopack detecta as mudanças de arquivo e recompila automaticamente. Não é necessário reiniciar o processo. Workers em segundo plano usando `tsc --watch` ou `nodemon` também percebem mudanças por meio de seus file watchers.

**`rebuild_triggers`:** Se `package.json` ou `yarn.lock` mudaram entre branches, os comandos `install` do serviço são executados novamente antes de o serviço reiniciar. Isso garante que as dependências estejam atualizadas após uma troca de branch que adicionou ou removeu pacotes.

**`exclude_paths`:** Acelera o bootstrap do worktree na primeira vez ao pular diretórios de que os serviços não precisam. Documentação, configurações de CI e scripts podem ser excluídos com segurança.

## Adaptando Esta Receita

**Sem worker em segundo plano:** Remova a seção `[services.worker]` e sua entrada em assign. O restante do Coastfile funciona sem alterações.

**Monorepo com múltiplas aplicações Next.js:** Adicione uma entrada `private_paths` para o diretório `.next` de cada app. Cada serviço bare recebe sua própria seção `[services.*]` com o `command` e a `port` apropriados.

**pnpm em vez de yarn:** Substitua `make yarn` pelo seu comando de instalação do pnpm. Ajuste o campo `cache` se o pnpm armazenar dependências em um local diferente (por exemplo, `.pnpm-store`).

**Sem serviços compartilhados:** Se você preferir bancos de dados por instância, remova as seções `[shared_services]` e `[secrets]`. Adicione Postgres e Redis a um `docker-compose.yml`, defina `compose` na seção `[coast]` e use [estratégias de volume](../coastfiles/VOLUMES.md) para controlar o isolamento. Use `strategy = "isolated"` para dados por instância ou `strategy = "shared"` para dados compartilhados.

**Provedores de autenticação adicionais:** Se sua biblioteca de autenticação usa variáveis de ambiente diferentes de `AUTH_URL` para URLs de callback, aplique o mesmo padrão `${WEB_DYNAMIC_PORT:-3000}` a essas variáveis no comando do serviço.

# Otimização de Bare Service

[Bear services](BARE_SERVICES.md) executam como processos simples dentro do contêiner Coast. Sem camadas Docker ou caches de imagem, o desempenho de inicialização e de troca de branch depende de como você estrutura seus comandos `install`, o cache e as estratégias de assign.

## Comandos de Instalação Rápidos

O campo `install` é executado antes de o serviço iniciar e novamente em cada `coast assign`. Se `install` executar incondicionalmente `make` ou `yarn install`, cada troca de branch pagará o custo completo da instalação mesmo quando nada tiver mudado.

**Use verificações condicionais para pular trabalho quando possível:**

```toml
[services.web]
install = "cd /workspace && (test -f node_modules/.yarn-state.yml || make yarn)"
command = "cd /workspace && yarn dev:web"
```

A proteção `test -f` pula a instalação se `node_modules` já existir. Na primeira execução ou após uma falha de cache, ela executa a instalação completa. Em assigns subsequentes, quando as dependências não tiverem mudado, ela termina instantaneamente.

Para binários compilados, verifique se a saída existe:

```toml
[services.zoekt]
install = "cd /workspace && (test -f bin/zoekt-webserver || make zoekt)"
command = "cd /workspace && ./bin/zoekt-webserver -index .sourcebot/index -rpc"
```

## Diretórios de Cache Entre Worktrees

Quando o Coast alterna uma instância de bare-service para um novo worktree, a montagem `/workspace` muda para um diretório diferente. Artefatos de build como `node_modules` ou binários compilados ficam para trás no worktree antigo. O campo `cache` informa ao Coast para preservar diretórios especificados entre trocas:

```toml
[services.web]
install = "cd /workspace && yarn install"
command = "cd /workspace && yarn dev"
cache = ["node_modules"]

[services.api]
install = "cd /workspace && make build"
command = "cd /workspace && ./bin/api-server"
cache = ["bin"]
```

Diretórios em cache são salvos antes da remontagem do worktree e restaurados depois. Isso significa que `yarn install` é executado incrementalmente em vez de do zero, e os binários compilados sobrevivem às trocas de branch.

## Isole Diretórios Por Instância com private_paths

Algumas ferramentas criam diretórios no workspace que contêm estado por processo: arquivos de lock, caches de build ou arquivos PID. Quando várias instâncias do Coast compartilham o mesmo workspace (mesma branch, sem worktree), esses diretórios entram em conflito.

O exemplo clássico é o Next.js, que cria um lock em `.next/dev/lock` na inicialização. Uma segunda instância do Coast vê o lock e se recusa a iniciar.

`private_paths` dá a cada instância seu próprio diretório isolado para os caminhos especificados:

```toml
[coast]
name = "my-app"
private_paths = ["packages/web/.next"]
```

Cada instância recebe uma montagem overlay por instância nesse caminho. Os arquivos de lock, caches de build e o estado do Turbopack ficam totalmente isolados. Nenhuma alteração de código é necessária.

Use `private_paths` para qualquer diretório em que instâncias concorrentes gravando nos mesmos arquivos causem problemas: `.next`, `.turbo`, `.parcel-cache`, arquivos PID ou bancos de dados SQLite.

## Conectando-se a Serviços Compartilhados

Quando você usa [serviços compartilhados](SHARED_SERVICES.md) para bancos de dados ou caches, os contêineres compartilhados são executados no daemon Docker do host, não dentro do Coast. Bare services executando dentro do Coast não conseguem alcançá-los via `localhost`.

Use `host.docker.internal` em vez disso:

```toml
[services.web]
command = "cd /workspace && DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

Você também pode usar [secrets](../coastfiles/SECRETS.md) para injetar strings de conexão como variáveis de ambiente:

```toml
[secrets.database_url]
extractor = "command"
run = "echo postgresql://postgres:postgres@host.docker.internal:5432/postgres"
inject = "env:DATABASE_URL"
```

Serviços Compose dentro do Coast não têm esse problema. O Coast roteia automaticamente os hostnames de serviços compartilhados por uma rede bridge para contêineres Compose. Isso afeta apenas bare services.

## Variáveis de Ambiente Inline

Comandos de bare service herdam variáveis de ambiente do contêiner Coast, incluindo qualquer coisa definida por arquivos `.env`, secrets e inject. Mas às vezes você precisa sobrescrever uma variável específica para um único serviço sem alterar arquivos de configuração compartilhados.

Prefixe o comando com atribuições inline:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} DATABASE_URL=postgresql://postgres:postgres@host.docker.internal:5432/postgres yarn dev:web"
```

Variáveis inline têm precedência sobre todo o resto. Isso é útil para:

- Definir `AUTH_URL` para a [porta dinâmica](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) para que redirecionamentos de autenticação funcionem em instâncias não checkoutadas
- Sobrescrever `DATABASE_URL` para apontar para um serviço compartilhado via `host.docker.internal`
- Definir flags específicas do serviço sem modificar arquivos `.env` compartilhados no workspace

## Estratégias de Assign para Bare Services

Escolha a [estratégia de assign](../coastfiles/ASSIGN.md) correta com base em como cada serviço detecta alterações de código:

| Strategy | When to use | Examples |
|---|---|---|
| `hot` | O serviço tem um observador de arquivos que detecta alterações automaticamente após a remontagem do worktree | Next.js (HMR), Vite, webpack, nodemon, tsc --watch |
| `restart` | O serviço carrega o código na inicialização e não observa alterações | Binários Go compilados, Rails, servidores Java |
| `none` | O serviço não depende do código do workspace ou usa um índice separado | Servidores de banco de dados, Redis, índices de busca |

```toml
[assign]
default = "none"

[assign.services]
web = "hot"
backend = "hot"
zoekt = "none"
```

Definir o padrão como `none` significa que serviços de infraestrutura nunca são tocados na troca de branch. Apenas os serviços que se importam com alterações de código são reiniciados ou dependem de hot reload.

## Veja Também

- [Bare Services](BARE_SERVICES.md) - a referência completa de bare services
- [Performance Optimizations](PERFORMANCE_OPTIMIZATIONS.md) - ajuste geral de desempenho, incluindo `exclude_paths` e `rebuild_triggers`
- [Dynamic Port Environment Variables](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - usando `WEB_DYNAMIC_PORT` e variáveis relacionadas em comandos

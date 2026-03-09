# Monorepo Full-Stack

Esta receita é para um monorepo grande com múltiplas aplicações web apoiadas por um banco de dados e uma camada de cache compartilhados. A stack usa Docker Compose para os serviços de backend mais pesados (Rails, Sidekiq, SSR) e executa servidores de desenvolvimento Vite como serviços bare no host DinD. Postgres e Redis rodam como serviços compartilhados no daemon Docker do host para que cada instância do Coast se comunique com a mesma infraestrutura sem duplicá-la.

Este padrão funciona bem quando:

- Seu monorepo contém vários apps que compartilham um banco de dados
- Você quer instâncias do Coast leves que não executem cada uma seu próprio Postgres e Redis
- Seus servidores de desenvolvimento frontend precisam ser acessíveis de dentro de containers do compose via `host.docker.internal`
- Você tem integrações MCP no host que se conectam a `localhost:5432` e quer que elas continuem funcionando sem alterações

## O Coastfile Completo

Aqui está o Coastfile completo. Cada seção é explicada em detalhes abaixo.

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]

[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"

# --- Bare services: Vite dev servers on the DinD host ---

[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"

# --- Shared services: Postgres and Redis on the host daemon ---

[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]

# --- Volumes: shared caches across all instances ---

[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"

# --- Secrets and injection ---

[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]

# --- Assign: branch-switch behavior ---

[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

## Projeto e Compose

```toml
[coast]
name = "acme"
compose = "./infra/docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "bash"]
run = ["npm install -g yarn"]
```

O campo `compose` aponta para o seu arquivo Docker Compose existente. O Coast executa `docker compose up -d` dentro do container DinD em `coast run`, então seus serviços de backend (servidores Rails, workers Sidekiq, processos SSR) iniciam automaticamente.

`[coast.setup]` instala pacotes no próprio host DinD — não dentro dos seus containers do compose. Eles são necessários para os serviços bare (servidores de desenvolvimento Vite) que rodam diretamente no host. Seus serviços do compose obtêm seus runtimes de seus Dockerfiles como de costume.

## Serviços Compartilhados

```toml
[shared_services.db]
image = "postgres:15.3-alpine"
ports = [5432]
volumes = ["infra_postgres:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "password" }

[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["infra_redis:/data"]
```

Postgres e Redis são declarados como [serviços compartilhados](../concepts_and_terminology/SHARED_SERVICES.md) em vez de rodarem dentro de cada Coast. Isso significa que eles rodam no daemon Docker do host, e cada instância do Coast se conecta a eles por uma rede bridge.

**Por que serviços compartilhados em vez de bancos de dados internos do compose?**

- **Instâncias mais leves.** Cada Coast deixa de subir seus próprios containers de Postgres e Redis, o que economiza memória e tempo de inicialização.
- **Reuso de volumes do host.** O campo `volumes` referencia seus volumes Docker existentes (os criados pelo seu `docker-compose up` local). Todos os dados que você já tem ficam imediatamente disponíveis — sem seeding, sem reexecutar migrações.
- **Compatibilidade com MCP.** Se você tem ferramentas MCP de banco de dados no host conectando a `localhost:5432`, elas continuam funcionando porque o Postgres compartilhado está no host nessa mesma porta. Nenhuma reconfiguração é necessária.

**O tradeoff:** não há isolamento de dados entre instâncias do Coast. Cada instância lê e escreve no mesmo banco de dados. Se o seu fluxo de trabalho precisa de bancos por instância, use [estratégias de volume](../concepts_and_terminology/VOLUMES.md) com `strategy = "isolated"` em vez disso, ou use `auto_create_db = true` no serviço compartilhado para obter um banco por instância dentro do Postgres compartilhado. Veja a [referência do Coastfile de Shared Services](../coastfiles/SHARED_SERVICES.md) para detalhes.

**A nomenclatura dos volumes importa.** Os nomes dos volumes (`infra_postgres`, `infra_redis`) devem corresponder aos volumes que já existem no seu host por executar `docker-compose up` localmente. Se eles não corresponderem, o serviço compartilhado iniciará com um volume vazio. Execute `docker volume ls` para verificar os nomes dos volumes existentes antes de escrever esta seção.

## Serviços Bare

```toml
[services.vite-web]
install = "cd /workspace && yarn install --immutable 2>/dev/null || yarn install"
command = "cd /workspace && yarn workspace @acme/web run dev"
port = 3040
restart = "on-failure"
cache = ["node_modules"]

[services.vite-api]
command = "cd /workspace && yarn workspace @acme/api run dev"
port = 3036
restart = "on-failure"
```

Servidores de desenvolvimento Vite são definidos como [serviços bare](../concepts_and_terminology/BARE_SERVICES.md) — processos simples rodando diretamente no host DinD, fora do Docker Compose. Este é o padrão de [tipos de serviço mistos](../concepts_and_terminology/MIXED_SERVICE_TYPES.md).

**Por que bare em vez de compose?**

A razão principal é rede. Serviços do compose que precisam alcançar o servidor de desenvolvimento Vite (para SSR, proxy de assets, ou conexões WebSocket de HMR) podem usar `host.docker.internal` para alcançar serviços bare no host DinD. Isso evita configurações complexas de rede do Docker e corresponde a como a maioria dos setups de monorepo configura `VITE_RUBY_HOST` ou variáveis de ambiente semelhantes.

Serviços bare também obtêm acesso direto ao sistema de arquivos bind-mounted em `/workspace` sem passar pelo overlay de um container interno. Isso significa que o file watcher do Vite reage mais rápido às mudanças.

**`install` e `cache`:** O campo `install` roda antes do serviço iniciar e novamente a cada `coast assign`. Aqui ele executa `yarn install` para capturar mudanças de dependência ao trocar de branch. O campo `cache` diz ao Coast para preservar `node_modules` ao alternar worktrees, para que as execuções de instalação sejam incrementais em vez de começarem do zero.

**Apenas um `install`:** Note que `vite-api` não tem campo `install`. Em um monorepo com yarn workspaces, um único `yarn install` na raiz instala dependências para todos os workspaces. Colocá-lo em apenas um serviço evita executá-lo duas vezes.

## Portas e Healthchecks

```toml
[ports]
api = 3000
web = 3002
vite-web = 3040
vite-api = 3036

[healthcheck]
web = "/"
api = "/"
```

Toda porta que você quer que o Coast gerencie vai em `[ports]`. Cada instância recebe uma [porta dinâmica](../concepts_and_terminology/PORTS.md) (faixa alta, sempre acessível) para cada porta declarada. A instância [checked-out](../concepts_and_terminology/CHECKOUT.md) também recebe a porta canônica (o número que você declarou) encaminhada para o host.

A seção `[healthcheck]` diz ao Coast como sondar a saúde de cada porta. Para portas com um caminho de healthcheck configurado, o Coast envia um HTTP GET a cada 5 segundos — qualquer resposta HTTP conta como saudável. Portas sem um caminho de healthcheck voltam para um check de conexão TCP (a porta consegue aceitar uma conexão?).

Neste exemplo, os servidores web Rails recebem healthchecks HTTP em `/` porque servem páginas HTML. Os servidores de desenvolvimento Vite ficam sem caminhos de healthcheck — eles não servem uma página raiz significativa, e um check TCP é suficiente para saber que estão aceitando conexões.

O status de healthcheck fica visível na UI do [Coastguard](../concepts_and_terminology/COASTGUARD.md) e via `coast ports`.

## Volumes

```toml
[volumes.bundle]
strategy = "shared"
service = "api-rails"
mount = "/usr/local/bundle"

[volumes.api_rails_cache]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/tmp/cache"

[volumes.api_assets]
strategy = "shared"
service = "api-rails"
mount = "/usr/src/api/public/assets"

[volumes.web_rails_cache]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/tmp/cache"

[volumes.web_assets]
strategy = "shared"
service = "web-rails"
mount = "/usr/src/web/public/assets"
```

Todos os volumes aqui usam `strategy = "shared"`, o que significa que um único volume Docker é compartilhado entre todas as instâncias do Coast. Esta é a escolha correta para **caches e artefatos de build** — coisas em que escritas concorrentes são seguras e duplicar por instância desperdiçaria espaço em disco e tornaria o startup mais lento:

- **`bundle`** — o cache de gems Ruby. As gems são as mesmas entre branches. Compartilhar evita baixar o bundle inteiro para cada instância do Coast.
- **`*_rails_cache`** — caches baseados em arquivo do Rails. Eles aceleram o desenvolvimento, mas não são preciosos — qualquer instância pode regenerá-los.
- **`*_assets`** — assets compilados. Mesma lógica dos caches.

**Por que não shared para bancos de dados?** O Coast imprime um aviso se você usar `strategy = "shared"` em um volume anexado a um serviço do tipo banco de dados. Múltiplos processos Postgres escrevendo no mesmo diretório de dados causam corrupção. Para bancos de dados, use [serviços compartilhados](../coastfiles/SHARED_SERVICES.md) (um Postgres no host, como esta receita faz) ou `strategy = "isolated"` (cada Coast recebe seu próprio volume). Veja a página de [Topologia de Volumes](../concepts_and_terminology/VOLUMES.md) para a matriz completa de decisão.

## Estratégias de Assign

```toml
[assign]
default = "none"
exclude_paths = [
    ".coasts",
    ".yarn",
    ".github",
    "docs",
    "scripts",
    "cli",
    "deploy",
    "tools",
    "mobile",
    "extensions",
]

[assign.services]
web-rails = "hot"
web-ssr = "hot"
web-sidekiq = "restart"
api-rails = "hot"
api-sidekiq = "restart"

[assign.rebuild_triggers]
web-rails = ["infra/Dockerfile", "web/Gemfile", "web/Gemfile.lock", "web/package.json"]
api-rails = ["infra/Dockerfile", "api/Gemfile", "api/Gemfile.lock", "api/package.json"]
```

A seção `[assign]` controla o que acontece com cada serviço quando você executa `coast assign` para alternar uma instância do Coast para uma worktree diferente. Acertar isso é a diferença entre uma troca de branch de 5 segundos e uma de 60 segundos.

### `default = "none"`

Definir o padrão como `"none"` significa que qualquer serviço não listado explicitamente em `[assign.services]` é deixado intocado na troca de branch. Isso é crítico para bancos de dados e caches — Postgres, Redis e serviços de infraestrutura não mudam entre branches e reiniciá-los é trabalho desperdiçado.

### Estratégias por serviço

| Serviço | Estratégia | Por quê |
|---|---|---|
| `web-rails`, `web-ssr`, `api-rails` | `hot` | Eles executam servidores de desenvolvimento com file watchers. O [remount do sistema de arquivos](../concepts_and_terminology/FILESYSTEM.md) troca o código sob `/workspace` e o watcher detecta mudanças automaticamente. Não é necessário reiniciar o container. |
| `web-sidekiq`, `api-sidekiq` | `restart` | Workers em background carregam o código na inicialização e não observam mudanças de arquivo. Eles precisam reiniciar o container para capturar o código da nova branch. |

Liste apenas serviços que estão realmente rodando. Se o seu `COMPOSE_PROFILES` inicia apenas um subconjunto de serviços, não liste os inativos — o Coast avalia a estratégia de assign para cada serviço listado, e reiniciar um serviço que não está rodando é trabalho desperdiçado. Veja [Otimizações de Performance](../concepts_and_terminology/PERFORMANCE_OPTIMIZATIONS.md) para mais sobre isso.

### `exclude_paths`

Esta é a otimização mais impactante para monorepos grandes. Ela diz ao Coast para pular árvores de diretórios inteiras durante a sincronização de arquivos ignorados pelo git (rsync) e o diff de `git ls-files` que rodam a cada assign.

O objetivo é excluir tudo o que seus serviços do Coast não precisam. Em um monorepo com 30.000 arquivos, os diretórios listados acima podem representar 8.000+ arquivos que são irrelevantes para os serviços em execução. Excluí-los reduz essa quantidade de stats de arquivo a cada troca de branch.

Para descobrir o que excluir, faça profiling do seu repo:

```bash
git ls-files | cut -d'/' -f1 | sort | uniq -c | sort -rn
```

Mantenha diretórios que contêm código-fonte montado em serviços em execução ou bibliotecas compartilhadas importadas por esses serviços. Exclua todo o resto — documentação, configs de CI, tooling, apps de outras equipes, clientes mobile, ferramentas de CLI e caches vendorizados como `.yarn`.

### `rebuild_triggers`

Sem triggers, um serviço com `strategy = "rebuild"` reconstrói sua imagem Docker em toda troca de branch — mesmo se nada que afete a imagem mudou. A seção `[assign.rebuild_triggers]` condiciona o rebuild a arquivos específicos.

Nesta receita, os serviços Rails normalmente usam `"hot"` (nenhum restart). Mas se alguém altera o Dockerfile ou Gemfile, os `rebuild_triggers` entram em ação e forçam um rebuild completo da imagem. Se nenhum dos arquivos de trigger mudou, o Coast pula o rebuild completamente. Isso evita builds de imagem caros em mudanças rotineiras de código, enquanto ainda captura mudanças em nível de infraestrutura.

## Secrets e Inject

```toml
[secrets.compose_profiles]
extractor = "command"
run = "echo api,web"
inject = "env:COMPOSE_PROFILES"

[secrets.uid]
extractor = "command"
run = "id -u"
inject = "env:UID"

[secrets.gid]
extractor = "command"
run = "id -g"
inject = "env:GID"

[inject]
env = ["USER", "BUNDLE_GEMS__CONTRIBSYS__COM"]
```

A seção `[secrets]` extrai valores em tempo de build e os injeta em instâncias do Coast como variáveis de ambiente.

- **`compose_profiles`** controla quais profiles do Docker Compose iniciam. É assim que você limita um Coast a rodar apenas os profiles `api` e `web` em vez de todos os serviços definidos no arquivo compose. Faça override no seu host com `export COMPOSE_PROFILES=api,web,portal` antes de buildar para mudar quais serviços iniciam.
- **`uid` / `gid`** passam o UID e o GID do usuário do host para dentro do container, o que é comum em setups Docker que precisam que a propriedade de arquivos corresponda entre host e container.

A seção `[inject]` é mais simples — ela encaminha variáveis de ambiente existentes do host para o container do Coast em runtime. Credenciais sensíveis como tokens do servidor de gems (`BUNDLE_GEMS__CONTRIBSYS__COM`) ficam no seu host e são encaminhadas sem serem gravadas em nenhum arquivo de configuração.

Para a referência completa sobre extractors de secrets e alvos de injeção, veja [Secrets](../coastfiles/SECRETS.md).

## Adaptando Esta Receita

**Stack de linguagem diferente:** Substitua os volumes específicos de Rails (bundle, cache do rails, assets) por equivalentes para sua stack — cache de módulos Go (`/go/pkg/mod`), cache do npm, cache do pip, etc. A estratégia continua `"shared"` para qualquer cache que seja seguro compartilhar entre instâncias.

**Menos apps:** Se seu monorepo tem apenas um app, remova as entradas de volume extras e simplifique `[assign.services]` para listar apenas seus serviços. Os padrões de serviços compartilhados e serviços bare ainda se aplicam.

**Bancos por instância:** Se você precisa de isolamento de dados entre instâncias do Coast, substitua `[shared_services.db]` por um Postgres interno do compose e adicione uma entrada em `[volumes]` com `strategy = "isolated"`. Cada instância recebe seu próprio volume de banco. Você pode fazer seed a partir do seu volume do host usando `snapshot_source` — veja a [referência do Coastfile de Volumes](../coastfiles/VOLUMES.md).

**Sem serviços bare:** Se seu frontend é totalmente containerizado e não precisa ser acessível via `host.docker.internal`, remova as seções `[services.*]` e `[coast.setup]`. Tudo roda via compose.

# Serviços Bare

Se você consegue conteinerizar seu projeto, você deve. Serviços bare existem para projetos que ainda não foram conteinerizados e em que adicionar um `Dockerfile` e `docker-compose.yml` não é prático no curto prazo.

Em vez de um `docker-compose.yml` orquestrando serviços conteinerizados, serviços bare permitem que você defina comandos de shell no seu Coastfile e o Coast os execute como processos comuns com um supervisor leve dentro do contêiner do Coast.

## Por que conteinerizar em vez disso

Os serviços do [Docker Compose](RUNTIMES_AND_SERVICES.md) oferecem:

- Builds reprodutíveis via Dockerfiles
- Health checks que o Coast pode aguardar durante a inicialização
- Isolamento de processos entre serviços
- Gerenciamento de volumes e rede feito pelo Docker
- Uma definição portátil que funciona em CI, staging e produção

Serviços bare não oferecem nada disso. Seus processos compartilham o mesmo sistema de arquivos, a recuperação de falhas é um loop de shell, e "funciona na minha máquina" é tão provável dentro do Coast quanto fora dele. Se o seu projeto já tem um `docker-compose.yml`, use-o.

## Quando serviços bare fazem sentido

- Você está adotando o Coast para um projeto que nunca foi conteinerizado e quer começar a obter valor do isolamento de worktree e do gerenciamento de portas imediatamente
- Seu projeto é uma ferramenta de processo único ou CLI em que um Dockerfile seria exagero
- Você quer conteinerizar gradualmente, começando com serviços bare e migrando para compose depois

## Configuração

Serviços bare são definidos com seções `[services.<name>]` no seu Coastfile. Um Coastfile pode definir serviços bare por conta própria ou junto com `compose` — veja [Tipos de Serviço Mistos](MIXED_SERVICE_TYPES.md) para o segundo caso.

```toml
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

Cada serviço tem quatro campos:

| Campo | Obrigatório | Descrição |
|---|---|---|
| `command` | sim | O comando de shell a executar (ex.: `"npm run dev"`) |
| `port` | não | A porta em que o serviço escuta, usada para mapeamento de portas |
| `restart` | não | Política de reinício: `"no"` (padrão), `"on-failure"` ou `"always"` |
| `install` | não | Um ou mais comandos para executar antes de iniciar (ex.: `"npm install"` ou `["npm install", "npm run build"]`) |

### Pacotes de setup

Como serviços bare rodam como processos comuns, o contêiner do Coast precisa ter os runtimes certos instalados. Use `[coast.setup]` para declarar pacotes do sistema:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

Eles são instalados antes de qualquer serviço iniciar. Sem isso, seus comandos `npm` ou `node` falharão dentro do contêiner.

### Comandos de instalação

O campo `install` roda antes do serviço iniciar e novamente a cada [`coast assign`](ASSIGN.md) (troca de branch). É aqui que entra a instalação de dependências:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

Os comandos de instalação rodam sequencialmente. Se algum comando de instalação falhar, o serviço não inicia.

### Políticas de reinício

- **`no`**: o serviço roda uma vez. Se ele sair, permanece morto. Use isto para tarefas one-shot ou serviços que você quer gerenciar manualmente.
- **`on-failure`**: reinicia o serviço se ele sair com um código diferente de zero. Saídas bem-sucedidas (código 0) são deixadas como estão. Usa backoff exponencial de 1 segundo até 30 segundos, e desiste após 10 falhas consecutivas.
- **`always`**: reinicia em qualquer saída, incluindo sucesso. Mesmo backoff que `on-failure`. Use isto para servidores de longa duração que nunca devem parar.

Se um serviço rodar por mais de 30 segundos antes de falhar, o contador de tentativas e o backoff são reiniciados — a suposição é que ele esteve saudável por um tempo e a falha é um problema novo.

## Como funciona

```text
┌─── Coast: dev-1 ──────────────────────────────────────┐
│                                                       │
│   /coast-supervisor/                                  │
│   ├── web.sh          (runs command, tracks PID)      │
│   ├── worker.sh                                       │
│   ├── start-all.sh    (launches all services)         │
│   ├── stop-all.sh     (SIGTERM via PID files)         │
│   └── ps.sh           (checks PID liveness)           │
│                                                       │
│   /var/log/coast-services/                            │
│   ├── web.log                                         │
│   └── worker.log                                      │
│                                                       │
│   No inner Docker daemon images are used.             │
│   Processes run directly on the container OS.         │
└───────────────────────────────────────────────────────┘
```

O Coast gera wrappers em shell script para cada serviço e os coloca em `/coast-supervisor/` dentro do contêiner DinD. Cada wrapper rastreia seu PID, redireciona a saída para um arquivo de log e implementa a política de reinício como um loop de shell. Não há Docker Compose, não há imagens Docker internas e não há isolamento em nível de contêiner entre serviços.

`coast ps` verifica a vivacidade do PID em vez de consultar o Docker, e `coast logs` acompanha (tail) os arquivos de log em vez de chamar `docker compose logs`. O formato de saída de log corresponde ao formato do compose `service | line` para que a UI do Coastguard funcione sem alterações.

## Portas

A configuração de portas funciona exatamente da mesma forma que com Coasts baseados em compose. Defina as portas em que seus serviços escutam em `[ports]`:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[Portas dinâmicas](PORTS.md) são alocadas em `coast run`, e [`coast checkout`](CHECKOUT.md) troca as portas canônicas como de costume. A única diferença é que não há uma rede Docker entre serviços — todos fazem bind diretamente no loopback do contêiner ou em `0.0.0.0`.

## Troca de branch

Quando você roda `coast assign` em um Coast com serviços bare, o seguinte acontece:

1. Todos os serviços em execução são parados via SIGTERM
2. O worktree muda para a nova branch
3. Os comandos de instalação são executados novamente (ex.: `npm install` pega as dependências da nova branch)
4. Todos os serviços reiniciam

Isso é equivalente ao que acontece com compose — `docker compose down`, troca de branch, rebuild, `docker compose up` — mas com processos de shell em vez de contêineres.

## Limitações

- **Sem health checks.** O Coast não consegue esperar que um serviço bare fique "saudável" da forma que consegue com um serviço do compose que define um health check. O Coast inicia o processo, mas não tem como saber quando ele está pronto.
- **Sem isolamento entre serviços.** Todos os processos compartilham o mesmo sistema de arquivos e o mesmo namespace de processos dentro do contêiner do Coast. Um serviço com mau comportamento pode afetar os outros.
- **Sem cache de build.** Builds do Docker Compose são cacheadas camada por camada. Comandos `install` de serviços bare rodam do zero a cada assign.
- **Recuperação de falhas é básica.** A política de reinício usa um loop de shell com backoff exponencial. Não é um supervisor de processos como systemd ou supervisord.
- **Sem `[omit]` ou `[unset]` para serviços.** A composição de tipos de Coastfile funciona com serviços compose, mas serviços bare não suportam omitir serviços individuais via Coastfiles tipados.

## Migração para Compose

Quando você estiver pronto para conteinerizar, o caminho de migração é simples:

1. Escreva um `Dockerfile` para cada serviço
2. Crie um `docker-compose.yml` que os referencie
3. Substitua as seções `[services.*]` no seu Coastfile por um campo `compose` apontando para o seu arquivo compose
4. Remova os pacotes de `[coast.setup]` que agora são tratados pelos seus Dockerfiles
5. Faça rebuild com [`coast build`](BUILDS.md)

Seus mapeamentos de portas, configuração de [volumes](VOLUMES.md), [serviços compartilhados](SHARED_SERVICES.md) e [segredos](SECRETS.md) todos são reaproveitados sem mudanças. A única coisa que muda é como os próprios serviços rodam.

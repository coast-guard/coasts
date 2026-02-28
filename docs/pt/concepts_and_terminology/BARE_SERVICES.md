# Serviços Bare

Se você consegue containerizar seu projeto, você deveria. Serviços bare existem para projetos que ainda não foram containerizados e em que adicionar um `Dockerfile` e um `docker-compose.yml` não é viável no curto prazo. Eles são um degrau, não um destino.

Em vez de um `docker-compose.yml` orquestrar serviços containerizados, serviços bare permitem que você defina comandos de shell no seu Coastfile e o Coast os executa como processos simples com um supervisor leve dentro do container do Coast.

## Por que Containerizar em vez disso

Os serviços do [Docker Compose](RUNTIMES_AND_SERVICES.md) oferecem:

- Builds reproduzíveis via Dockerfiles
- Health checks que o Coast pode aguardar durante a inicialização
- Isolamento de processos entre serviços
- Gerenciamento de volumes e rede feito pelo Docker
- Uma definição portátil que funciona em CI, staging e produção

Serviços bare não oferecem nada disso. Seus processos compartilham o mesmo sistema de arquivos, a recuperação de falhas é um loop de shell, e “funciona na minha máquina” é tão provável dentro do Coast quanto fora dele. Se o seu projeto já tem um `docker-compose.yml`, use-o.

## Quando Serviços Bare Fazem Sentido

- Você está adotando o Coast para um projeto que nunca foi containerizado e quer começar a obter valor do isolamento de worktree e do gerenciamento de portas imediatamente
- Seu projeto é uma ferramenta de processo único ou CLI em que um Dockerfile seria exagero
- Você quer iterar a containerização gradualmente — comece com serviços bare e mude para compose depois

## Configuração

Serviços bare são definidos com seções `[services.<name>]` no seu Coastfile. Um Coastfile **não pode** definir tanto `compose` quanto `[services]` — eles são mutuamente exclusivos.

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

### Pacotes de Setup

Como serviços bare rodam como processos simples, o container do Coast precisa ter os runtimes corretos instalados. Use `[coast.setup]` para declarar pacotes de sistema:

```toml
[coast.setup]
packages = ["nodejs", "npm"]
```

Eles são instalados antes de qualquer serviço iniciar. Sem isso, seus comandos `npm` ou `node` vão falhar dentro do container.

### Comandos de Instalação

O campo `install` roda antes do serviço iniciar e novamente a cada [`coast assign`](ASSIGN.md) (troca de branch). É aqui que entra a instalação de dependências:

```toml
[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
```

Os comandos de instalação são executados sequencialmente. Se qualquer comando de instalação falhar, o serviço não inicia.

### Políticas de Reinício

- **`no`** — o serviço executa uma vez. Se sair, permanece morto. Use isto para tarefas pontuais (one-shot) ou serviços que você quer gerenciar manualmente.
- **`on-failure`** — reinicia o serviço se ele sair com um código diferente de zero. Saídas bem-sucedidas (código 0) são deixadas como estão. Usa backoff exponencial de 1 segundo até 30 segundos, e desiste após 10 falhas consecutivas.
- **`always`** — reinicia em qualquer saída, incluindo sucesso. Mesmo backoff de `on-failure`. Use isto para servidores de longa execução que nunca deveriam parar.

Se um serviço rodar por mais de 30 segundos antes de falhar, o contador de tentativas e o backoff são redefinidos — a suposição é que ele esteve saudável por um tempo e a falha é um novo problema.

## Como Funciona por Baixo dos Panos

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

O Coast gera wrappers em shell script para cada serviço e os coloca em `/coast-supervisor/` dentro do container DinD. Cada wrapper acompanha seu PID, redireciona a saída para um arquivo de log e implementa a política de reinício como um loop de shell. Não há Docker Compose, não há imagens Docker internas, e não há isolamento em nível de container entre serviços.

`coast ps` verifica se o PID está vivo em vez de consultar o Docker, e `coast logs` faz tail dos arquivos de log em vez de chamar `docker compose logs`. O formato de saída dos logs corresponde ao formato do compose `service | line` para que a UI do Coastguard funcione sem mudanças.

## Portas

A configuração de portas funciona exatamente da mesma forma que com Coasts baseados em compose. Defina as portas em que seus serviços escutam em `[ports]`:

```toml
[services.web]
command = "npm start"
port = 3000

[ports]
web = 3000
```

[Portas dinâmicas](PORTS.md) são alocadas em `coast run`, e [`coast checkout`](CHECKOUT.md) troca as portas canônicas como de costume. A única diferença é que não há rede Docker entre serviços — todos eles fazem bind diretamente no loopback do container ou em `0.0.0.0`.

## Troca de Branch

Quando você executa `coast assign` em um Coast de serviços bare, o seguinte acontece:

1. Todos os serviços em execução são interrompidos via SIGTERM
2. A worktree muda para o novo branch
3. Os comandos de instalação são executados novamente (ex.: `npm install` captura as dependências do novo branch)
4. Todos os serviços reiniciam

Isto é equivalente ao que acontece com compose — `docker compose down`, troca de branch, rebuild, `docker compose up` — mas com processos de shell em vez de containers.

## Limitações

- **Sem health checks.** O Coast não pode aguardar que um serviço bare fique “saudável” da mesma forma que pode com um serviço compose que define um health check. Ele inicia o processo e torce pelo melhor.
- **Sem isolamento entre serviços.** Todos os processos compartilham o mesmo sistema de arquivos e o mesmo namespace de processos dentro do container do Coast. Um serviço com mau comportamento pode afetar os outros.
- **Sem cache de build.** Builds do Docker Compose são cacheados camada por camada. Comandos `install` de serviços bare executam do zero a cada assign.
- **Recuperação de falhas é básica.** A política de reinício usa um loop de shell com backoff exponencial. Não é um supervisor de processos como systemd ou supervisord.
- **Sem `[omit]` ou `[unset]` para serviços.** A composição de tipos de Coastfile funciona com serviços compose, mas serviços bare não suportam omitir serviços individuais via Coastfiles tipados.

## Migração para Compose

Quando você estiver pronto para containerizar, o caminho de migração é direto:

1. Escreva um `Dockerfile` para cada serviço
2. Crie um `docker-compose.yml` que os referencie
3. Substitua as seções `[services.*]` no seu Coastfile por um campo `compose` apontando para seu arquivo compose
4. Remova pacotes de `[coast.setup]` que agora são tratados pelos seus Dockerfiles
5. Rebuild com [`coast build`](BUILDS.md)

Seus mapeamentos de portas, [volumes](VOLUMES.md), [serviços compartilhados](SHARED_SERVICES.md) e configuração de [secrets](SECRETS.md) são todos mantidos sem alterações. A única coisa que muda é como os serviços em si rodam.

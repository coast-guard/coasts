# Arquitetura

Um coast remoto divide a execução entre sua máquina local e um servidor remoto. A experiência do desenvolvedor permanece inalterada porque o daemon roteia de forma transparente cada operação por meio de um túnel SSH.

## A Divisão em Dois Contêineres

Cada coast remoto cria dois contêineres:

### Shell Coast (local)

Um contêiner Docker leve na sua máquina. Ele tem os mesmos bind mounts de um coast normal (`/host-project`, `/workspace`), mas sem daemon Docker interno e sem serviços compose. Seu entrypoint é `sleep infinity`.

O shell coast existe por um motivo: ele preserva a [ponte de sistema de arquivos](../concepts_and_terminology/FILESYSTEM.md) para que agentes e editores no lado do host possam editar arquivos em `/workspace`. Essas edições são sincronizadas para o remoto por meio de [rsync e mutagen](FILE_SYNC.md).

### Remote Coast (remoto)

Gerenciado por `coast-service` na máquina remota. É aqui que o trabalho real acontece: um contêiner DinD completo executando seus serviços compose, com portas dinâmicas alocadas para cada serviço.

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                            │
│                                                                          │
│  ┌────────────┐    unix     ┌───────────────────────────────────────┐    │
│  │ coast CLI  │───socket───▶│ coast-daemon                         │    │
│  └────────────┘             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shell Coast (sleep infinity)    │  │    │
│                             │  │ - /host-project (bind mount)    │  │    │
│                             │  │ - /workspace (mount --bind)     │  │    │
│                             │  │ - NO inner docker               │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Port Manager                    │  │    │
│                             │  │ - allocates local dynamic ports │  │    │
│                             │  │ - SSH -L tunnels to remote      │  │    │
│                             │  │   dynamic ports                 │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shared Services (local)         │  │    │
│                             │  │ - postgres, redis, etc.         │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  state.db (shadow instance,           │    │
│                             │           remote_host, port allocs)   │    │
│                             └───────────────────┬───────────────────┘    │
│                                                 │                        │
│                                    SSH tunnel   │  rsync / SSH           │
│                                                 │                        │
└─────────────────────────────────────────────────┼────────────────────────┘
                                                  │
┌─────────────────────────────────────────────────┼────────────────────────┐
│ REMOTE MACHINE                                  │                        │
│                                                 ▼                        │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │ coast-service (HTTP API on :31420)                                │   │
│  │                                                                   │   │
│  │  ┌───────────────────────────────────────────────────────────┐    │   │
│  │  │ DinD Container (per instance)                             │    │   │
│  │  │  /workspace (synced from local)                           │    │   │
│  │  │  compose services / bare services                         │    │   │
│  │  │  published on dynamic ports (e.g. :52340 -> :3000)        │    │   │
│  │  └───────────────────────────────────────────────────────────┘    │   │
│  │                                                                   │   │
│  │  Port Manager (dynamic port allocation per instance)              │   │
│  │  Build artifacts (/data/images/)                                  │   │
│  │  Image cache (/data/image-cache/)                                 │   │
│  │  Keystore (encrypted secrets)                                     │   │
│  │  remote-state.db (instances, worktrees)                           │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## Camada de Túnel SSH

O daemon faz a ponte entre local e remoto usando dois tipos de túneis SSH:

### Túneis de Encaminhamento (local para remoto)

Para cada porta de serviço, o daemon cria um túnel `ssh -L` que mapeia uma porta dinâmica local para a porta dinâmica remota correspondente. É isso que faz com que `localhost:{dynamic_port}` alcance o serviço remoto.

```text
ssh -N -L {local_dynamic}:localhost:{remote_dynamic} user@remote
```

Quando você executa `coast ports`, a coluna dynamic mostra esses endpoints locais do túnel.

### Túneis Reversos (remoto para local)

[Serviços compartilhados](../concepts_and_terminology/SHARED_SERVICES.md) (Postgres, Redis, etc.) são executados na sua máquina local. O daemon cria túneis `ssh -R` para que o contêiner DinD remoto possa alcançá-los:

```text
ssh -N -R 0.0.0.0:{remote_port}:localhost:{local_port} user@remote
```

Dentro do contêiner DinD remoto, os serviços se conectam aos serviços compartilhados por meio de `host.docker.internal:{port}`, que é resolvido para o gateway da bridge do Docker onde o túnel reverso está escutando.

O sshd do host remoto deve ter `GatewayPorts clientspecified` habilitado para que túneis reversos façam bind em `0.0.0.0` em vez de `127.0.0.1`.

### Recuperação de Túneis

Os túneis SSH podem quebrar quando seu laptop entra em suspensão ou a rede muda. O daemon executa um loop de verificação em segundo plano que:

1. Sonda cada porta dinâmica a cada 5 segundos por meio de conexão TCP.
2. Se todas as portas de uma instância estiverem indisponíveis, encerra os processos de túnel obsoletos dessa instância e os restabelece.
3. Se apenas algumas portas estiverem indisponíveis (falha parcial), restabelece somente os túneis ausentes sem interromper os saudáveis.
4. Limpa bindings de porta remotos obsoletos por meio de `fuser -k` antes de criar novos túneis reversos.

A recuperação é por instância -- recuperar os túneis de uma instância nunca interrompe os de outra.

## Cadeia de Encaminhamento de Portas

Todas as portas são dinâmicas na camada intermediária. Portas canônicas só existem nos endpoints: dentro do contêiner DinD onde os serviços escutam, e no seu localhost por meio de [`coast checkout`](../concepts_and_terminology/CHECKOUT.md).

```text
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

Essa cadeia de três saltos permite múltiplas instâncias do mesmo projeto em uma única máquina remota sem conflitos de porta. Cada instância recebe seu próprio conjunto de portas dinâmicas em ambos os lados.

## Roteamento de Requisições

Todo handler do daemon verifica `remote_host` na instância. Se estiver definido, a requisição é encaminhada para o coast-service por meio do túnel SSH:

| Command | Remote behavior |
|---------|-----------------|
| `coast run` | Criar shell coast localmente + transferir artefatos + encaminhar para coast-service |
| `coast build` | Compilar na máquina remota (sem encaminhamento de build local) |
| `coast assign` | Fazer rsync do novo conteúdo da worktree + encaminhar requisição assign |
| `coast exec` | Encaminhar para coast-service |
| `coast ps` | Encaminhar para coast-service |
| `coast logs` | Encaminhar para coast-service |
| `coast stop` | Encaminhar + encerrar túneis SSH locais |
| `coast start` | Encaminhar + restabelecer túneis SSH |
| `coast rm` | Encaminhar + encerrar túneis + excluir instância shadow local |
| `coast checkout` | Somente local (socat no host, sem necessidade de encaminhamento) |
| `coast secret set` | Armazenar localmente + encaminhar para o keystore remoto |

## coast-service

`coast-service` é o plano de controle em execução na máquina remota. Ele é um servidor HTTP (Axum) escutando na porta 31420 que espelha as operações locais do daemon: build, run, assign, exec, ps, logs, stop, start, rm, secrets e reinicializações de serviços.

Ele gerencia seu próprio banco de dados de estado SQLite, contêineres Docker (DinD), alocação dinâmica de portas, artefatos de build, cache de imagens e keystore criptografado. O daemon se comunica com ele exclusivamente por meio do túnel SSH -- o coast-service nunca é exposto à internet pública.

Consulte [Setup](SETUP.md) para instruções de implantação.

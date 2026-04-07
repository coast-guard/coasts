# Sincronização de Arquivos

Coasts remotos usam uma estratégia de sincronização em duas camadas: rsync para transferências em massa, mutagen para sincronização contínua em tempo real. Ambas as ferramentas são dependências de tempo de execução instaladas dentro dos contêineres coast -- elas não são necessárias na sua máquina host.

## Onde a Sincronização É Executada

```text
Local Machine                          Remote Machine
┌─────────────────────────────┐        ┌──────────────────────────────┐
│  coastd daemon              │        │                              │
│    │                        │        │                              │
│    │ rsync (direct SSH)     │  SSH   │  /data/workspaces/{p}/{i}/   │
│    │────────────────────────│───────▶│    (rsync writes here)       │
│    │                        │        │    │                         │
│    │ docker exec            │        │    │ bind mount              │
│    ▼                        │        │    ▼                         │
│  Shell Container            │  SSH   │  Remote DinD Container       │
│    /workspace (bind mount)  │───────▶│    /workspace                │
│    mutagen (continuous sync)│        │    (compose services running)│
│    SSH key (copied in)      │        │                              │
└─────────────────────────────┘        └──────────────────────────────┘
```

O daemon executa o rsync diretamente a partir do processo host. O mutagen é executado dentro do contêiner shell local via `docker exec`.

## Camada 1: rsync (Transferência em Massa)

Em `coast run` e `coast assign`, o daemon executa rsync do host para transferir os arquivos do workspace para o remoto:

```bash
rsync -rlDzP --delete-after \
  --rsync-path="sudo rsync" \
  --exclude '.git' --exclude 'node_modules' \
  --exclude 'target' --exclude '__pycache__' \
  --exclude '.react-router' --exclude '.next' \
  -e "ssh -p {port} -i {key}" \
  {local_workspace}/ {user}@{host}:{remote_workspace}/
```

Depois que o rsync é concluído, o daemon executa `sudo chown -R` no remoto para dar ao usuário SSH a propriedade dos arquivos. O rsync é executado como root via `--rsync-path="sudo rsync"` porque o workspace remoto pode conter arquivos pertencentes ao root oriundos de operações do coast-service dentro do contêiner.

### O que o rsync faz bem

- **Transferências iniciais.** O primeiro `coast run` envia o workspace inteiro.
- **Trocas de worktree.** `coast assign` envia apenas o delta entre o worktree antigo e o novo. Arquivos que não mudaram não são retransmitidos.
- **Compressão.** A flag `-z` comprime os dados em trânsito.

### Caminhos excluídos

O rsync ignora caminhos que não devem ser transferidos:

| Path | Why |
|------|-----|
| `.git` | Grande, não é necessário no remoto (o conteúdo do worktree é suficiente) |
| `node_modules` | Reconstruído dentro do DinD a partir dos lockfiles |
| `target` | Artefatos de build Rust/Go, reconstruídos no remoto |
| `__pycache__` | Cache de bytecode Python, regenerado |
| `.react-router` | Tipos gerados, recriados pelo servidor de desenvolvimento |
| `.next` | Cache de build do Next.js, regenerado |

### Protegendo arquivos gerados

Quando `coast assign` é executado com `--delete-after`, o rsync normalmente exclui no remoto os arquivos que não existem localmente. Isso destruiria arquivos gerados (como clientes proto em `generated/`) que o servidor de desenvolvimento remoto criou, mas que seu worktree local não contém.

Para evitar isso, o rsync usa regras `--filter 'P generated/***'` que protegem diretórios gerados específicos contra exclusão. Os caminhos protegidos incluem `generated/`, `.react-router/`, `internal/generated/` e `app/generated/`.

### Tratamento de transferência parcial

O código de saída 23 do rsync (transferência parcial) é tratado como um aviso não fatal. Isso lida com uma condição de corrida em que servidores de desenvolvimento em execução dentro do DinD remoto regeneram arquivos (por exemplo, `.react-router/types/`) enquanto o rsync está gravando. Os arquivos-fonte são transferidos com sucesso; apenas artefatos gerados podem falhar, e esses são regenerados pelo servidor de desenvolvimento de qualquer forma.

## Camada 2: mutagen (Sincronização Contínua)

Após o rsync inicial, o daemon inicia uma sessão mutagen dentro do contêiner shell local:

```bash
docker exec {shell_container} mutagen sync create \
    --name coast-{project}-{instance} \
    --sync-mode one-way-safe \
    --ignore-vcs \
    --ignore node_modules --ignore target \
    --ignore __pycache__ --ignore .next \
    /workspace/ {user}@{host}:{remote_workspace}/
```

O mutagen observa mudanças em arquivos via eventos no nível do sistema operacional (inotify dentro do contêiner), agrupa mudanças e transfere deltas por uma conexão SSH persistente. Suas edições aparecem no remoto em segundos.

### Modo one-way-safe

O mutagen é executado no modo `one-way-safe`: as mudanças fluem apenas do local para o remoto. Arquivos criados no remoto (por servidores de desenvolvimento, ferramentas de build etc.) não são sincronizados de volta para sua máquina local. Isso evita que artefatos gerados poluam seu diretório de trabalho.

### Mutagen é uma dependência de tempo de execução

O mutagen é instalado em:

- A **imagem coast** (construída por `coast build` a partir de `[coast.setup]`), usada pelo contêiner shell local.
- A **imagem Docker coast-service** (`Dockerfile.coast-service`), usada no lado remoto.

O daemon nunca executa mutagen diretamente no host. Ele orquestra via `docker exec` para dentro do contêiner shell.

## Ciclo de Vida

| Command | rsync | mutagen |
|---------|-------|---------|
| `coast run` | Transferência completa inicial | Sessão criada após o rsync |
| `coast assign` | Transferência delta do novo worktree | Sessão antiga encerrada, nova sessão criada |
| `coast stop` | -- | Sessão encerrada |
| `coast rm` | -- | Sessão encerrada |

### Comportamento de fallback

Se a sessão mutagen falhar ao iniciar dentro do contêiner shell, o daemon registra um aviso. O rsync inicial ainda fornece o conteúdo do workspace, mas as mudanças de arquivos não serão sincronizadas em tempo real até que a sessão seja restabelecida (por exemplo, no próximo `coast assign` ou na reinicialização do daemon).

## Configuração da Estratégia de Sincronização

A seção `[remote]` do seu Coastfile controla a estratégia de sincronização:

```toml
[remote]
workspace_sync = "mutagen"    # "rsync" (default) or "mutagen"
```

- **`rsync`** (padrão): apenas a transferência inicial do rsync é executada. Sem sincronização contínua. Bom para ambientes de CI ou jobs em lote onde a sincronização em tempo real não é necessária.
- **`mutagen`**: rsync para a transferência inicial, depois mutagen para sincronização contínua. Use isso para desenvolvimento interativo em que você quer que as edições apareçam no remoto imediatamente.

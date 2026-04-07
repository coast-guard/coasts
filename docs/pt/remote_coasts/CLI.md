# CLI e Configuração

Esta página cobre o grupo de comandos `coast remote`, o formato de configuração `Coastfile.remote` e o gerenciamento de disco para máquinas remotas.

## Comandos de Gerenciamento Remoto

### `coast remote add`

Registre uma máquina remota no daemon:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

Exemplos:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

Os detalhes da conexão são armazenados no `state.db` do daemon. Eles nunca são armazenados em Coastfiles.

### `coast remote ls`

Liste todos os remotos registrados:

```bash
coast remote ls
```

### `coast remote rm`

Remova um remoto registrado:

```bash
coast remote rm <name>
```

Se ainda houver instâncias em execução no remoto, remova-as primeiro com `coast rm`.

### `coast remote test`

Verifique a conectividade SSH e a disponibilidade do coast-service:

```bash
coast remote test <name>
```

Isso verifica o acesso SSH, confirma que o coast-service está acessível na porta 31420 através do túnel SSH e informa a arquitetura do remoto e a versão do coast-service.

### `coast remote prune`

Limpe recursos órfãos em uma máquina remota:

```bash
coast remote prune <name>              # remove orphaned resources
coast remote prune <name> --dry-run    # preview what would be removed
```

O prune identifica recursos órfãos por meio de referência cruzada entre volumes Docker e diretórios de workspace com o banco de dados de instâncias do coast-service. Recursos pertencentes a instâncias ativas nunca são removidos.

## Configuração do Coastfile

Coasts remotos usam um Coastfile separado que estende sua configuração base. O nome do arquivo determina o tipo:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### Exemplo mínimo

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### A seção `[remote]`

A seção `[remote]` declara preferências de sincronização. Os detalhes de conexão (host, usuário, chave SSH) vêm de `coast remote add` e são resolvidos em tempo de execução.

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | Estratégia de sincronização: `"rsync"` para transferência em massa única apenas, `"mutagen"` para rsync + sincronização contínua em tempo real |

### Restrições de validação

1. A seção `[remote]` é obrigatória quando o tipo de Coastfile começa com `remote`.
2. Coastfiles não remotos não podem ter uma seção `[remote]`.
3. Configuração de host inline não é suportada. Os detalhes de conexão devem vir de um remoto registrado.
4. Volumes compartilhados com `strategy = "shared"` criam um volume Docker no host remoto, compartilhado entre todos os coasts nesse remoto. O volume não é distribuído entre diferentes máquinas remotas.

### Herança

Coastfiles remotos usam o mesmo [sistema de herança](../coastfiles/INHERITANCE.md) que outros Coastfiles tipados. A diretiva `extends = "Coastfile"` mescla a configuração base com as substituições remotas. Você pode substituir portas, serviços, volumes e atribuir estratégias como em qualquer outra variante tipada.

## Gerenciamento de Disco

### Uso de recursos por instância

Cada instância de coast remoto consome aproximadamente:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

Disco mínimo recomendado: **50 GB** para projetos típicos com 2-3 instâncias simultâneas.

### Convenções de nomenclatura de recursos

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### Limpeza em `coast rm`

Quando `coast rm` remove uma instância remota, ele limpa:

1. O contêiner DinD remoto (via coast-service)
2. O volume Docker DinD (`coast-dind--{project}--{name}`)
3. O diretório de workspace (`/data/workspaces/{project}/{name}`)
4. O registro local da instância sombra, as alocações de porta e o contêiner de shell

### Quando executar prune

Se `df -h` no remoto mostrar alto uso de disco após remover instâncias, recursos órfãos podem ter ficado para trás devido a operações com falha ou interrompidas. Execute `coast remote prune` para recuperar espaço:

```bash
# See what would be removed
coast remote prune my-vm --dry-run

# Actually remove
coast remote prune my-vm
```

O prune nunca remove recursos pertencentes a instâncias ativas.

# Builds Remotos

Builds remotos são executados na máquina remota via coast-service. Isso garante que o build use a arquitetura nativa do remoto (por exemplo, x86_64 em uma instância EC2), independentemente da sua arquitetura local (por exemplo, ARM Mac). Não é necessária compilação cruzada nem emulação de arquitetura.

## Como Funciona

Quando você executa `coast build --type remote`, o seguinte acontece:

1. O daemon sincroniza via rsync os arquivos-fonte do projeto (Coastfile, compose.yml, Dockerfiles, inject/) para o workspace remoto via SSH.
2. O daemon chama `POST /build` no coast-service através do túnel SSH.
3. O coast-service executa o build completo nativamente no remoto: `docker build`, pulling de imagens, cache de imagens e extração de segredos, tudo em `/data/images/`.
4. O coast-service retorna um `BuildResponse` com o caminho do artefato e os metadados do build.
5. O daemon sincroniza via rsync o diretório completo do artefato (coastfile.toml, compose.yml, manifest.json, secrets/, inject/, image tarballs) de volta para `~/.coast/images/{project}/{build_id}/` na sua máquina local.
6. O daemon cria um symlink `latest-remote` apontando para o novo build.

```text
Local Machine                              Remote Machine
┌─────────────────────────────┐            ┌───────────────────────────┐
│  ~/.coast/images/my-app/    │            │  /data/images/my-app/     │
│    latest-remote -> {id}    │  ◀─rsync─  │    {id}/                  │
│    {id}/                    │            │      manifest.json        │
│      manifest.json          │            │      coastfile.toml       │
│      coastfile.toml         │            │      compose.yml          │
│      compose.yml            │            │      *.tar (images)       │
│      *.tar (images)         │            │                           │
└─────────────────────────────┘            └───────────────────────────┘
```

## Comandos

```bash
# Build on the default remote (auto-selected if only one registered)
coast build --type remote

# Build on a specific remote
coast build --type remote --remote my-vm

# Build without running (standalone)
coast build --type remote
```

`coast run --type remote` também dispara um build se ainda não existir nenhum build compatível.

## Correspondência de Arquitetura

O `manifest.json` de cada build registra a arquitetura para a qual ele foi construído (por exemplo, `aarch64`, `x86_64`). Quando você executa `coast run --type remote`, o daemon verifica se um build existente corresponde à arquitetura do remoto de destino:

- **A arquitetura corresponde**: o build é reutilizado. Não é necessário rebuild.
- **A arquitetura não corresponde**: o daemon procura o build mais recente com a arquitetura correta. Se nenhum existir, ele retorna um erro com orientação para reconstruir.

Isso significa que você pode construir uma vez em um remoto x86_64 e fazer deploy em qualquer número de remotos x86_64 sem reconstruir. Mas você não pode usar um build ARM em um remoto x86_64, nem vice-versa.

## Symlinks

Builds remotos usam um symlink separado dos builds locais:

| Symlink | Aponta para |
|---------|-----------|
| `latest` | Build local mais recente |
| `latest-remote` | Build remoto mais recente |
| `latest-{type}` | Build local mais recente de um tipo específico de Coastfile |

A separação impede que um build remoto sobrescreva seu symlink local `latest`, ou vice-versa.

## Auto-Pruning

O Coast mantém até 5 builds remotos por par `(coastfile_type, architecture)`. Após cada build remoto bem-sucedido, os builds mais antigos que excederem o limite são removidos automaticamente.

Builds que estão em uso por instâncias em execução nunca são removidos, independentemente do limite. Se você tiver 7 builds remotos x86_64, mas 3 deles estiverem sustentando instâncias ativas, todos os 3 estarão protegidos.

A remoção é sensível à arquitetura: se você tiver builds remotos `aarch64` e `x86_64`, cada arquitetura mantém seu próprio conjunto de 5 builds de forma independente.

## Armazenamento de Artefatos

Os artefatos de build remoto são armazenados em dois lugares:

| Location | Path | Purpose |
|----------|------|---------|
| Remote | `/data/images/{project}/{build_id}/` | Fonte da verdade na máquina remota |
| Local | `~/.coast/images/{project}/{build_id}/` | Cache local para reutilização entre remotos |

O cache de imagens em `/data/image-cache/` no remoto é compartilhado entre todos os projetos, assim como `~/.coast/image-cache/` localmente.

## Relação com Builds Locais

Builds remotos e builds locais são independentes. Um `coast build` (sem `--type remote`) sempre constrói na sua máquina local e atualiza o symlink `latest`. Um `coast build --type remote` sempre constrói na máquina remota e atualiza o symlink `latest-remote`.

Você pode ter builds locais e remotos do mesmo projeto coexistindo. Coasts locais usam builds locais; coasts remotos usam builds remotos.

Para mais informações sobre como builds funcionam em geral (estrutura do manifest, cache de imagens, builds tipados), veja [Builds](../concepts_and_terminology/BUILDS.md).

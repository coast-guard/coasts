# Builds

Pense em um build do coast como uma imagem Docker com ajuda extra. Um build é um artefato baseado em diretório que agrupa tudo o que é necessário para criar instâncias do Coast: um [Coastfile](COASTFILE_TYPES.md) resolvido, um arquivo compose reescrito, tarballs de imagens OCI previamente baixadas e arquivos do host injetados. Ele não é uma imagem Docker em si, mas contém imagens Docker (como tarballs) além dos metadados de que o Coast precisa para conectá-las.

## O que `coast build` faz

Quando você executa `coast build`, o daemon executa estas etapas em ordem:

1. Analisa e valida o Coastfile.
2. Lê o arquivo compose e filtra os serviços omitidos.
3. Extrai [secrets](SECRETS.md) dos extratores configurados e os armazena criptografados no keystore.
4. Constrói imagens Docker para serviços do compose que têm diretivas `build:` (no host).
5. Baixa imagens Docker para serviços do compose que têm diretivas `image:`.
6. Armazena em cache todas as imagens como tarballs OCI em `~/.coast/image-cache/`.
7. Se `[coast.setup]` estiver configurado, constrói uma imagem base DinD personalizada com os pacotes, comandos e arquivos especificados.
8. Escreve o diretório do artefato de build com o manifesto, o coastfile resolvido, o compose reescrito e os arquivos injetados.
9. Atualiza o link simbólico `latest` para apontar para o novo build.
10. Remove automaticamente builds antigos além do limite de retenção.

## Onde os builds ficam

```text
~/.coast/
  images/
    my-project/
      latest -> a3c7d783_20260227143000       (symlink)
      a3c7d783_20260227143000/                (versioned build)
        manifest.json
        coastfile.toml
        compose.yml
        inject/
      b4d8e894_20260226120000/                (older build)
        ...
  image-cache/                                (shared tarball cache)
    postgres_16_a1b2c3d4e5f6.tar
    redis_7_f6e5d4c3b2a1.tar
    coast-built_my-project_web_latest_...tar
```

Cada build recebe um **ID de build** exclusivo no formato `{coastfile_hash}_{YYYYMMDDHHMMSS}`. O hash incorpora o conteúdo do Coastfile e a configuração resolvida, então alterações no Coastfile produzem um novo ID de build.

O link simbólico `latest` sempre aponta para o build mais recente para resolução rápida. Se o seu projeto usa Coastfiles tipados (por exemplo, `Coastfile.light`), cada tipo recebe seu próprio link simbólico: `latest-light`.

O cache de imagens em `~/.coast/image-cache/` é compartilhado entre todos os projetos. Se dois projetos usarem a mesma imagem Postgres, o tarball será armazenado em cache uma única vez.

## O que um build contém

Cada diretório de build contém:

- **`manifest.json`** -- metadados completos do build: nome do projeto, timestamp do build, hash do coastfile, lista de imagens em cache/construídas, nomes de secrets, serviços omitidos, [estratégias de volume](VOLUMES.md) e mais.
- **`coastfile.toml`** -- o Coastfile resolvido (mesclado com o pai se estiver usando `extends`).
- **`compose.yml`** -- uma versão reescrita do seu arquivo compose em que diretivas `build:` são substituídas por tags de imagem pré-construídas, e serviços omitidos são removidos.
- **`inject/`** -- cópias de arquivos do host de `[inject].files` (por exemplo, `~/.gitconfig`, `~/.npmrc`).

## Builds não contêm secrets

Secrets são extraídos durante a etapa de build, mas são armazenados em um keystore criptografado separado em `~/.coast/keystore.db` -- não dentro do diretório do artefato de build. O manifesto registra apenas os **nomes** dos secrets que foram extraídos, nunca os valores.

Isso significa que os artefatos de build são seguros para inspeção sem expor dados sensíveis. Secrets são descriptografados e injetados depois, quando uma instância Coast é criada com `coast run`.

## Builds e Docker

Um build envolve três tipos de imagens Docker:

- **Imagens construídas** -- serviços do compose com diretivas `build:` são construídos no host via `docker build`, marcados como `coast-built/{project}/{service}:latest` e salvos como tarballs no cache de imagens.
- **Imagens baixadas** -- serviços do compose com diretivas `image:` são baixados e salvos como tarballs.
- **Imagem Coast** -- se `[coast.setup]` estiver configurado, uma imagem Docker personalizada é construída sobre `docker:dind` com os pacotes, comandos e arquivos especificados. Marcada como `coast-image/{project}:{build_id}`.

Em tempo de execução ([`coast run`](RUN.md)), esses tarballs são carregados no [daemon DinD interno](RUNTIMES_AND_SERVICES.md) via `docker load`. É isso que faz as instâncias Coast iniciarem rapidamente sem precisar baixar imagens de um registry.

## Builds e instâncias

Quando você executa [`coast run`](RUN.md), o Coast resolve o build mais recente (ou um `--build-id` específico) e usa seus artefatos para criar a instância. O ID do build é registrado na instância.

Você não precisa reconstruir para criar mais instâncias. Um build pode servir muitas instâncias Coast em execução em paralelo.

## Quando reconstruir

Reconstrua apenas quando seu Coastfile, `docker-compose.yml` ou configuração de infraestrutura mudar. Reconstruir consome muitos recursos -- isso baixa novamente imagens, reconstrói imagens Docker e reextrai secrets.

Alterações no código não exigem reconstrução. O Coast monta seu diretório de projeto diretamente em cada instância, então atualizações de código são refletidas imediatamente.

## Remoção automática

O Coast mantém até 5 builds por tipo de Coastfile. Após cada `coast build` bem-sucedido, builds antigos além do limite são removidos automaticamente.

Builds que estão em uso por instâncias em execução nunca são removidos, independentemente do limite. Se você tiver 7 builds mas 3 deles estiverem sustentando instâncias ativas, todos os 3 estarão protegidos.

## Remoção manual

Você pode remover builds manualmente via `coast rm-build` ou pela aba Builds do Coastguard.

- **Remoção completa do projeto** (`coast rm-build <project>`) exige que todas as instâncias sejam primeiro paradas e removidas. Isso remove todo o diretório de build, imagens Docker associadas, volumes e containers.
- **Remoção seletiva** (por ID de build, disponível na UI do Coastguard) ignora builds que estão em uso por instâncias em execução.

## Builds tipados

Se o seu projeto usa vários Coastfiles (por exemplo, `Coastfile` para a configuração padrão e `Coastfile.snap` para volumes inicializados por snapshot), cada tipo mantém seu próprio link simbólico `latest-{type}` e seu próprio conjunto de remoção de 5 builds.

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

A remoção automática de um build `snap` nunca afeta builds `default`, e vice-versa.

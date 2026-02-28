# Builds

Pense em um build do Coast como uma imagem Docker com ajuda extra. Um build é um artefato baseado em diretório que agrupa tudo o que é necessário para criar instâncias do Coast: um [Coastfile](COASTFILE_TYPES.md) resolvido, um arquivo compose reescrito, tarballs de imagens OCI pré-baixadas e arquivos do host injetados. Ele não é uma imagem Docker em si, mas contém imagens Docker (como tarballs) além dos metadados de que o Coast precisa para conectá-las entre si.

## What `coast build` Does

Quando você executa `coast build`, o daemon executa estas etapas em ordem:

1. Analisa e valida o Coastfile.
2. Lê o arquivo compose e filtra serviços omitidos.
3. Extrai [secrets](SECRETS.md) de extratores configurados e os armazena criptografados no keystore.
4. Faz build de imagens Docker para serviços do compose que têm diretivas `build:` (no host).
5. Faz pull de imagens Docker para serviços do compose que têm diretivas `image:`.
6. Armazena em cache todas as imagens como tarballs OCI em `~/.coast/image-cache/`.
7. Se `[coast.setup]` estiver configurado, faz build de uma imagem base DinD personalizada com os pacotes, comandos e arquivos especificados.
8. Escreve o diretório do artefato de build com o manifesto, coastfile resolvido, compose reescrito e arquivos injetados.
9. Atualiza o symlink `latest` para apontar para o novo build.
10. Faz auto-prune de builds antigos além do limite de retenção.

## Where Builds Live

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

Cada build recebe um **build ID** único no formato `{coastfile_hash}_{YYYYMMDDHHMMSS}`. O hash incorpora o conteúdo do Coastfile e a configuração resolvida, então mudanças no Coastfile produzem um novo build ID.

O symlink `latest` sempre aponta para o build mais recente para resolução rápida. Se o seu projeto usa Coastfiles tipados (por exemplo, `Coastfile.light`), cada tipo recebe seu próprio symlink: `latest-light`.

O cache de imagens em `~/.coast/image-cache/` é compartilhado entre todos os projetos. Se dois projetos usam a mesma imagem do Postgres, o tarball é armazenado em cache uma vez.

## What a Build Contains

Cada diretório de build contém:

- **`manifest.json`** -- metadados completos do build: nome do projeto, timestamp do build, hash do coastfile, lista de imagens em cache/construídas, nomes de secrets, serviços omitidos, [estratégias de volume](VOLUMES.md) e mais.
- **`coastfile.toml`** -- o Coastfile resolvido (mesclado com o pai se estiver usando `extends`).
- **`compose.yml`** -- uma versão reescrita do seu arquivo compose em que diretivas `build:` são substituídas por tags de imagem pré-buildadas, e serviços omitidos são removidos.
- **`inject/`** -- cópias de arquivos do host de `[inject].files` (por exemplo, `~/.gitconfig`, `~/.npmrc`).

## Builds Do Not Contain Secrets

Secrets são extraídos durante a etapa de build, mas são armazenados em um keystore criptografado separado em `~/.coast/keystore.db` -- não dentro do diretório do artefato de build. O manifesto registra apenas os **nomes** dos secrets que foram extraídos, nunca os valores.

Isso significa que artefatos de build são seguros para inspecionar sem expor dados sensíveis. Os secrets são descriptografados e injetados depois, quando uma instância do Coast é criada com `coast run`.

## Builds and Docker

Um build envolve três tipos de imagens Docker:

- **Imagens buildadas** -- serviços do compose com diretivas `build:` são buildados no host via `docker build`, marcados (tagged) como `coast-built/{project}/{service}:latest` e salvos como tarballs no cache de imagens.
- **Imagens puxadas (pulled)** -- serviços do compose com diretivas `image:` são baixados (pulled) e salvos como tarballs.
- **Imagem do Coast** -- se `[coast.setup]` estiver configurado, uma imagem Docker personalizada é buildada em cima de `docker:dind` com os pacotes, comandos e arquivos especificados. Marcada (tagged) como `coast-image/{project}:{build_id}`.

Em runtime (`coast run`), esses tarballs são carregados no daemon interno de [DinD](RUNTIMES_AND_SERVICES.md) via `docker load`. É isso que faz as instâncias do Coast iniciarem rapidamente sem precisar baixar imagens de um registry.

## Builds and Instances

Quando você executa `coast run`, o Coast resolve o build mais recente (ou um `--build-id` específico) e usa seus artefatos para criar a instância. O build ID é registrado na instância.

Você não precisa reconstruir para criar mais instâncias. Um build pode servir muitas instâncias do Coast executando em paralelo.

## When to Rebuild

Só reconstrua quando seu Coastfile, `docker-compose.yml` ou a configuração de infraestrutura mudarem. Reconstruir é intensivo em recursos -- ele baixa novamente imagens, refaz o build de imagens Docker e reextrai secrets.

Mudanças de código não exigem um rebuild. O Coast monta o diretório do seu projeto diretamente em cada instância, então atualizações de código são capturadas imediatamente.

## Auto-Pruning

O Coast mantém até 5 builds por tipo de Coastfile. Após cada `coast build` bem-sucedido, builds mais antigos além do limite são removidos automaticamente.

Builds que estão em uso por instâncias em execução nunca são podados (pruned), independentemente do limite. Se você tem 7 builds mas 3 deles estão sustentando instâncias ativas, todos os 3 são protegidos.

## Manual Removal

Você pode remover builds manualmente via `coast rm-build` ou pela aba Builds do Coastguard.

- **Remoção completa do projeto** (`coast rm-build <project>`) exige que todas as instâncias sejam interrompidas e removidas primeiro. Ela remove todo o diretório de build, imagens Docker associadas, volumes e containers.
- **Remoção seletiva** (por build ID, disponível na UI do Coastguard) ignora builds que estão em uso por instâncias em execução.

## Typed Builds

Se o seu projeto usa múltiplos Coastfiles (por exemplo, `Coastfile` para a configuração padrão e `Coastfile.snap` para volumes semeados por snapshot), cada tipo mantém seu próprio symlink `latest-{type}` e seu próprio pool de pruning de 5 builds.

```bash
coast build              # uses Coastfile, updates "latest"
coast build --type snap  # uses Coastfile.snap, updates "latest-snap"
```

Podar (prune) um build `snap` nunca toca em builds `default`, e vice-versa.

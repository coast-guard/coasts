# Volumes de SSG

Dentro de `[shared_services.<name>]`, o array `volumes` usa a sintaxe padrão do Docker Compose:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/postgres:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

Uma `/` inicial significa um **caminho de bind do host** -- os bytes ficam no sistema de arquivos do host e o serviço interno os lê e grava diretamente no local. Sem uma barra inicial, por exemplo `pg_wal:/var/lib/postgresql/wal`, a origem é um **volume nomeado do Docker que vive dentro do daemon Docker aninhado do SSG** -- ele sobrevive a `coast ssg rm` e é removido por `coast ssg rm --with-data`. Ambas as formas são aceitas.

Rejeitados na análise: caminhos relativos (`./data:/...`), componentes `..`, volumes apenas de contêiner (sem origem) e alvos duplicados dentro de um mesmo serviço.

## Reutilizando um volume Docker de docker-compose ou de um serviço compartilhado inline

Se você já tem dados dentro de um volume nomeado do Docker no host -- de `docker-compose up`, de um inline `[shared_services.postgres] volumes = ["infra_postgres_data:/..."]`, ou de um `docker volume create` feito manualmente -- você pode fazer o SSG ler os mesmos bytes montando por bind o diretório subjacente do host desse volume:

```toml
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }
```

O lado esquerdo é o caminho no sistema de arquivos do host de um volume Docker existente; `docker volume inspect <name>` o informa como o campo `Mountpoint`. O Coast não copia bytes -- o SSG lê e grava os mesmos arquivos que o docker-compose usou. `coast ssg rm` (sem `--with-data`) deixa o volume intocado, então o docker-compose também pode continuar usando-o.

> **Por que não simplesmente `infra_postgres_data:/var/lib/postgresql/data`?** Isso funciona para `[shared_services.*]` inline (o volume é criado no daemon Docker do host, onde o docker-compose pode vê-lo). Isso *não* funciona da mesma forma dentro de um SSG -- um nome sem barra inicial cria um volume novo dentro do daemon Docker aninhado do SSG, isolado do host. Use o caminho do ponto de montagem do volume em vez disso quando quiser compartilhar dados com qualquer coisa que rode no daemon do host.

### `coast ssg import-host-volume`

`coast ssg import-host-volume` resolve o `Mountpoint` do volume via `docker volume inspect` e emite (ou aplica) a linha `volumes` equivalente, para que você não precise montar manualmente o caminho `/var/lib/docker/volumes/<name>/_data`.

O modo snippet (padrão) imprime o fragmento TOML para colar:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data
```

A saída é um bloco `[shared_services.postgres]` com a nova entrada `volumes = [...]` já mesclada:

```text
# Add the following to Coastfile.shared_service_groups (infra_postgres_data -> /var/lib/postgresql/data):

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = [
    "/var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data",
]
env = { POSTGRES_PASSWORD = "coast" }

# Bind line: /var/lib/docker/volumes/infra_postgres_data/_data:/var/lib/postgresql/data
```

O modo apply reescreve `Coastfile.shared_service_groups` no local e salva o original em `Coastfile.shared_service_groups.bak`:

```bash
coast ssg import-host-volume infra_postgres_data \
    --service postgres \
    --mount /var/lib/postgresql/data \
    --apply
```

Flags:

- `<VOLUME>` (posicional) -- volume nomeado do Docker no host. Já deve existir (a verificação é `docker volume inspect`); caso contrário, crie ou renomeie primeiro com `docker volume create`.
- `--service` -- a seção `[shared_services.<name>]` a editar. A seção já deve existir.
- `--mount` -- caminho absoluto no contêiner. Caminhos relativos são rejeitados. Caminhos de montagem duplicados no mesmo serviço são erros fatais.
- `--file` / `--working-dir` / `--config` -- descoberta do Coastfile do SSG, mesmas regras de `coast ssg build`.
- `--apply` -- reescreve o Coastfile no local. Não pode ser combinado com `--config` (texto inline não tem para onde ser gravado de volta).

O arquivo `.bak` contém os bytes originais literalmente, então você pode recuperar o estado exato anterior ao apply.

`/var/lib/docker/volumes/<name>/_data` é o caminho que o Docker usa como ponto de montagem de volume há muitos anos e é o que `docker volume inspect` informa hoje. O Docker não promete formalmente manter esse caminho para sempre; se uma versão futura do Docker mover os volumes para outro lugar, execute `coast ssg import-host-volume` novamente para obter o novo caminho.

## Permissões

Várias imagens se recusam a iniciar quando seu diretório de dados pertence ao usuário errado. Postgres (UID 999 na tag debian, UID 70 na tag alpine), MySQL/MariaDB (UID 999) e MongoDB (UID 999) são os infratores mais comuns. Se o diretório do host pertencer ao root, o Postgres sai na inicialização com uma mensagem seca "data directory has wrong ownership".

A correção é um único comando:

```bash
# postgres:16 (debian)
sudo chown -R 999:999 /var/coast-data/postgres

# postgres:16-alpine
sudo chown -R 70:70 /var/coast-data/postgres
```

Execute isso antes de `coast ssg run`. Se o diretório ainda não existir, `coast ssg run` o cria com a propriedade padrão (root no Linux, seu usuário no macOS via Docker Desktop). Esse padrão geralmente é incorreto para o Postgres. Se você chegou aqui via `coast ssg import-host-volume` e o `docker-compose up` já tinha feito `chown` no volume na primeira inicialização, então já está tudo certo.

## `coast ssg doctor`

`coast ssg doctor` é uma verificação somente leitura executada contra o SSG do projeto atual (resolvido a partir do `[coast].name` do `Coastfile` no cwd ou de `--working-dir`). Ele imprime um resultado por par `(service, host-bind)` na build ativa, além de resultados de extração de segredos (veja [Secrets](SECRETS.md)).

Para cada imagem conhecida (Postgres, MySQL, MariaDB, MongoDB), ele consulta uma tabela embutida de UID/GID, compara com `stat(2)` em cada caminho do host e emite:

- `ok` quando o proprietário corresponde ao esperado pela imagem.
- `warn` quando diverge. A mensagem inclui o comando `chown` para corrigir.
- `info` quando o diretório ainda não existe, ou quando a imagem correspondente tem apenas volumes nomeados (nada para verificar do lado do host).

Serviços cujas imagens não estão na tabela de imagens conhecidas são ignorados silenciosamente. Forks como `ghcr.io/baosystems/postgis` não são sinalizados -- o doctor prefere não dizer nada a emitir um aviso incorreto.

```bash
coast ssg doctor
```

Saída de exemplo com um diretório do Postgres com proprietário incorreto:

```text
SSG 'b455787d95cfdeb_20260420061903' (project cg): 1 warning(s), 0 ok, 0 info. Fix the warnings before `coast ssg run`.

  LEVEL   SERVICE              PATH                                     MESSAGE
  warn    postgres             /var/coast-data/postgres                 Owner 0:0 but postgres expects 999:999. Run `sudo chown -R 999:999 /var/coast-data/postgres` before `coast ssg run`.
```

O doctor não modifica nada. Permissões sobre bytes que você colocou no sistema de arquivos do host não são algo que o Coast altere silenciosamente.

## Notas de plataforma

- **macOS Docker Desktop.** Caminhos brutos do host devem ser listados em Settings -> Resources -> File Sharing. Os padrões incluem `/Users`, `/Volumes`, `/private`, `/tmp`. `/var/coast-data` **não** está na lista padrão no macOS -- prefira `$HOME/coast-data/...` para caminhos novos, ou adicione `/var/coast-data` ao File Sharing. A forma `/var/lib/docker/volumes/<name>/_data` *não* é um caminho do host -- o Docker o resolve dentro da sua própria VM -- então funciona sem uma entrada em File Sharing.
- **WSL2.** Prefira caminhos nativos do WSL (`~`, `/mnt/wsl/...`). `/mnt/c/...` funciona, mas é lento por causa do protocolo 9P que faz a ponte com o sistema de arquivos do host Windows.
- **Linux.** Sem pegadinhas.

## Ciclo de vida

- `coast ssg rm` -- remove o contêiner DinD externo do SSG. **O conteúdo dos volumes permanece intocado**, o conteúdo dos bind mounts do host permanece intocado, o keystore permanece intocado. Qualquer outra coisa que use o mesmo volume Docker continua funcionando.
- `coast ssg rm --with-data` -- remove volumes que vivem **dentro do daemon Docker aninhado do SSG** (a forma `name:path` sem barra inicial). Bind mounts do host e volumes Docker externos ainda permanecem intocados -- o Coast não é dono deles.
- `coast ssg build` -- nunca toca em volumes. Apenas grava um manifesto e (quando `[secrets]` é declarado) linhas no keystore.
- `coast ssg run` / `start` / `restart` -- cria diretórios de bind mount no host se eles não existirem (com a propriedade padrão -- veja [Permissões](#permissions)).

## Veja também

- [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md) -- esquema TOML completo, incluindo a sintaxe de volume
- [Volume Topology](../concepts_and_terminology/VOLUMES.md) -- estratégias de volume compartilhado, isolado e semeado por snapshot para serviços que não são SSG
- [Building](BUILDING.md) -- de onde vem o manifesto
- [Lifecycle](LIFECYCLE.md) -- quando volumes são criados, parados e removidos
- [Secrets](SECRETS.md) -- segredos injetados por arquivo vão para `~/.coast/ssg/runs/<project>/secrets/<basename>` e são montados por bind nos serviços internos como somente leitura

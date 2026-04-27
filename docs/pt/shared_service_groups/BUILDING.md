# Construindo um Grupo de Serviço Compartilhado

`coast ssg build` analisa o `Coastfile.shared_service_groups` do seu projeto, extrai quaisquer segredos declarados, puxa cada imagem para o cache de imagens do host e grava um artefato de build versionado em `~/.coast/ssg/<project>/builds/<build_id>/`. O comando é não destrutivo em relação a um SSG já em execução -- o próximo `coast ssg run` ou `coast ssg start` utiliza o novo build, mas um `<project>-ssg` em execução continua servindo seu build atual até que você o reinicie.

O nome do projeto vem de `[coast].name` no `Coastfile` irmão. Cada projeto tem seu próprio SSG chamado `<project>-ssg`, seu próprio diretório de build e seu próprio `latest_build_id` -- não existe um "SSG atual" em todo o host.

Para o esquema TOML completo, veja [Coastfile: Shared Service Groups](../coastfiles/SHARED_SERVICE_GROUPS.md).

## Descoberta

`coast ssg build` encontra seu Coastfile usando as mesmas regras que `coast build`:

- Sem flags, ele procura no diretório de trabalho atual por `Coastfile.shared_service_groups` ou `Coastfile.shared_service_groups.toml`. Ambas as formas são equivalentes e o sufixo `.toml` tem prioridade quando ambos existem.
- `-f <path>` / `--file <path>` aponta para um arquivo arbitrário.
- `--working-dir <dir>` desacopla a raiz do projeto da localização do Coastfile (mesma flag que `coast build --working-dir`).
- `--config '<inline-toml>'` oferece suporte a fluxos de script e CI em que você sintetiza o Coastfile inline.

```bash
coast ssg build
coast ssg build -f /path/to/Coastfile.shared_service_groups
coast ssg build --working-dir /shared/coast
coast ssg build --config '[shared_services.pg]
image = "postgres:16"
ports = [5432]'
```

O build resolve o nome do projeto a partir do `Coastfile` irmão no mesmo diretório. Se você usar `--config` (sem um Coastfile.shared_service_groups em disco), o cwd ainda deve conter um `Coastfile` cujo `[coast].name` seja o projeto do SSG.

## O que o Build Faz

Cada `coast ssg build` transmite o progresso pelo mesmo canal `BuildProgressEvent` que `coast build`, então a CLI renderiza contadores de etapas `[N/M]`.

1. **Analisa** o `Coastfile.shared_service_groups`. `[ssg]`, `[shared_services.*]`, `[secrets.*]` e `[unset]` são as seções de nível superior aceitas. As entradas de volume são divididas em bind mounts do host e volumes nomeados internos (veja [Volumes](VOLUMES.md)).
2. **Resolve o build id.** O id tem o formato `{coastfile_hash}_{YYYYMMDDHHMMSS}`. O hash incorpora a fonte bruta, um resumo determinístico dos serviços analisados e a configuração `[secrets.*]` (portanto, editar o `extractor` ou `var` de um segredo produz um novo id).
3. **Sintetiza o `compose.yml` interno.** Cada bloco `[shared_services.*]` se torna uma entrada em um único arquivo Docker Compose. Esse é o arquivo que o daemon Docker interno do SSG executa via `docker compose up -d` no momento de `coast ssg run`.
4. **Extrai segredos.** Quando `[secrets.*]` não está vazio, executa cada extractor declarado e armazena o resultado criptografado em `~/.coast/keystore.db` sob `coast_image = "ssg:<project>"`. Ignorado silenciosamente quando o Coastfile não tem bloco `[secrets]`. Veja [Secrets](SECRETS.md) para o pipeline completo.
5. **Puxa e armazena em cache cada imagem.** As imagens são armazenadas como tarballs OCI em `~/.coast/image-cache/`, o mesmo pool que `coast build` usa. Acertos de cache de qualquer um dos comandos aceleram o outro.
6. **Grava o artefato de build** em `~/.coast/ssg/<project>/builds/<build_id>/` com três arquivos: `manifest.json`, `ssg-coastfile.toml` e `compose.yml` (veja o layout abaixo).
7. **Atualiza o `latest_build_id` do projeto.** Isso é um sinalizador no banco de dados de estado, não um symlink do sistema de arquivos. `coast ssg run` e `coast ssg ps` o leem para saber em qual build operar.
8. **Faz auto-prune** dos builds mais antigos, mantendo os 5 mais recentes para este projeto. Diretórios de artefatos anteriores em `~/.coast/ssg/<project>/builds/` são removidos do disco. Builds fixados (veja "Fixando um projeto a um build específico" abaixo) são sempre preservados.

## Layout do Artefato

```text
~/.coast/
  keystore.db                                          (compartilhado, com namespace por coast_image)
  keystore.key
  image-cache/                                         (pool compartilhado de tarballs OCI)
  ssg/
    cg/                                                (projeto "cg")
      builds/
        b455787d95cfdeb_20260420061903/                (o novo build)
          manifest.json
          ssg-coastfile.toml
          compose.yml
        a1c7d783e4f56c9a_20260419184221/               (build anterior)
          ...
    filemap/                                           (projeto "filemap" -- árvore separada)
      builds/
        ...
    runs/
      cg/                                              (scratch de execução por projeto)
        compose.override.yml                           (renderizado em coast ssg run)
        secrets/<basename>                             (segredos injetados por arquivo, modo 0600)
```

`manifest.json` captura os metadados do build que importam para o código downstream:

```json
{
  "build_id": "b455787d95cfdeb_20260420061903",
  "built_at": "2026-04-20T06:19:03Z",
  "coastfile_hash": "b455787d95cfdeb",
  "services": [
    {
      "name": "postgres",
      "image": "postgres:16",
      "ports": [5432],
      "env_keys": ["POSTGRES_USER", "POSTGRES_DB"],
      "volumes": ["pg_data:/var/lib/postgresql/data"],
      "auto_create_db": true
    }
  ],
  "secret_injects": [
    {
      "secret_name": "pg_password",
      "inject_type": "env",
      "inject_target": "POSTGRES_PASSWORD",
      "services": ["postgres"]
    }
  ]
}
```

Valores de env e payloads de segredos estão intencionalmente ausentes -- apenas os nomes das variáveis de ambiente e os *targets* de injeção são capturados. Os valores dos segredos vivem criptografados no keystore, nunca nos arquivos de artefato.

`ssg-coastfile.toml` é o Coastfile analisado, interpolado e pós-validação. Ele é byte a byte idêntico ao que o daemon teria visto no momento da análise. Útil para auditar um build passado.

`compose.yml` é o que o daemon Docker interno do SSG executa. Veja [Volumes](VOLUMES.md) para as regras de síntese, especialmente a estratégia de bind mount de caminho simétrico.

## Inspecionando um Build Sem Executá-lo

`coast ssg ps` lê `manifest.json` diretamente para o `latest_build_id` do projeto -- ele não inspeciona nenhum contêiner. Você pode executá-lo imediatamente após `coast ssg build` para ver os serviços que iniciarão no próximo `coast ssg run`:

```bash
coast ssg ps

# SSG build: b455787d95cfdeb_20260420061903 (project: cg)
#
#   SERVICE              IMAGE                          PORT       STATUS
#   postgres             postgres:16                    5432       built
#   redis                redis:7-alpine                 6379       built
```

A coluna `PORT` é a porta do contêiner interno. Portas dinâmicas do host são alocadas em `coast ssg run`; a porta virtual voltada ao consumidor é reportada por `coast ssg ports`. Veja [Routing](ROUTING.md) para o panorama completo.

Para navegar por todos os builds de um projeto (com timestamps, contagens de serviço e qual build é atualmente o latest), use:

```bash
coast ssg builds-ls
```

## Rebuilds

Um novo `coast ssg build` é a forma canônica de atualizar um SSG. Ele reextrai segredos (se houver), atualiza `latest_build_id` e remove artefatos antigos. Consumidores não fazem rebuild automático -- suas referências `from_group = true` são resolvidas no momento do build do consumidor em relação a qualquer build que estivesse atual então. Para mover um consumidor para um SSG mais novo, execute `coast build` para o consumidor.

O runtime é tolerante entre rebuilds: portas virtuais permanecem estáveis por `(project, service, container_port)`, então os consumidores não precisam ser atualizados para o roteamento. Mudanças de forma (um serviço foi renomeado ou removido) aparecem como erros de conexão no nível do consumidor, não como uma mensagem de "drift" no nível do Coast. Veja [Routing](ROUTING.md) para o motivo.

## Fixando um projeto a um build específico

Por padrão, o SSG executa o `latest_build_id` do projeto. Se você precisar congelar um projeto em um build anterior -- para reproduzir uma regressão, comparar A/B dois builds entre worktrees ou manter uma branch de longa duração em uma forma conhecida como boa -- use os comandos de fixação:

```bash
coast ssg checkout-build <build_id>     # fixa este projeto em <build_id>
coast ssg show-pin                      # reporta a fixação ativa (se houver)
coast ssg uncheckout-build              # libera a fixação; volta para latest
```

As fixações são por projeto consumidor (uma fixação por projeto, compartilhada entre worktrees). Quando fixado:

- `coast ssg run` inicia automaticamente o build fixado em vez de `latest_build_id`.
- `coast build` valida referências `from_group` em relação ao manifesto do build fixado.
- `auto_prune` não excluirá o diretório do build fixado, mesmo que ele fique fora da janela dos 5 mais recentes.

A SPA do Coastguard mostra um badge `PINNED` ao lado do build id quando uma fixação está ativa, e `LATEST` quando não está. Os comandos de fixação também aparecem em [CLI](CLI.md).

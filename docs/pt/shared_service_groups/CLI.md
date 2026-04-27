# Referência da CLI `coast ssg`

Todo subcomando `coast ssg` se comunica com o mesmo daemon local pelo socket Unix existente. `coast shared-service-group` é um alias para `coast ssg`.

A maioria dos verbos resolve um projeto a partir de `[coast].name` do `Coastfile` no cwd (ou `--working-dir <dir>`). Apenas `coast ssg ls` é entre projetos.

Todos os comandos aceitam um sinalizador global `--silent` / `-s` que suprime a saída de progresso e imprime apenas o resumo final ou erros.

## Comandos

### Build e inspeção

| Command | Summary |
|---------|---------|
| `coast ssg build [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Analisa `Coastfile.shared_service_groups`, extrai quaisquer `[secrets.*]`, baixa imagens, grava o artefato em `~/.coast/ssg/<project>/builds/<id>/`, atualiza `latest_build_id`, remove builds antigos. Veja [Building](BUILDING.md). |
| `coast ssg ps` | Mostra a lista de serviços do build SSG deste projeto (lê `manifest.json` mais o estado dos containers em execução). Veja [Lifecycle -> ps](LIFECYCLE.md#coast-ssg-ps). |
| `coast ssg builds-ls [--working-dir <dir>] [-f <file>]` | Lista todo artefato de build em `~/.coast/ssg/<project>/builds/` com timestamp, contagem de serviços e anotações `(latest)` / `(pinned)`. |
| `coast ssg ls` | Listagem entre projetos de todo SSG conhecido pelo daemon (projeto, status, id do build, contagem de serviços, criado em). Veja [Lifecycle -> ls](LIFECYCLE.md#coast-ssg-ls). |

### Ciclo de vida

| Command | Summary |
|---------|---------|
| `coast ssg run` | Cria o DinD `<project>-ssg`, aloca portas dinâmicas do host, materializa segredos (quando declarados), inicializa a stack compose interna. Veja [Lifecycle -> run](LIFECYCLE.md#coast-ssg-run). |
| `coast ssg start` | Inicia um SSG previamente criado, mas parado. Re-materializa segredos e recria quaisquer socats de checkout de porta canônica preservados. |
| `coast ssg stop [--force]` | Para o DinD SSG do projeto. Preserva o container, portas dinâmicas, portas virtuais e linhas de checkout. `--force` desmonta primeiro túneis SSH remotos. |
| `coast ssg restart` | Para + inicia. Preserva o container e as portas dinâmicas. |
| `coast ssg rm [--with-data] [--force]` | Remove o DinD SSG do projeto. `--with-data` remove volumes nomeados internos. `--force` prossegue apesar de consumidores shadow remotos. Conteúdos de bind-mount do host nunca são tocados. **O keystore nunca é tocado** -- use `coast ssg secrets clear` para isso. |

### Logs e exec

| Command | Summary |
|---------|---------|
| `coast ssg logs [--service <name>] [--tail N] [--follow]` | Transmite logs do DinD externo ou de um serviço interno. `--follow` transmite até Ctrl+C. |
| `coast ssg exec [--service <name>] -- <cmd...>` | Executa no container externo `<project>-ssg` ou em um serviço interno. Tudo após `--` é repassado literalmente. |

### Roteamento e checkout

| Command | Summary |
|---------|---------|
| `coast ssg ports` | Mostra o mapeamento de portas canônica / dinâmica / virtual por serviço com anotação `(checked out)` quando aplicável. Veja [Routing](ROUTING.md). |
| `coast ssg checkout [--service <name> \| --all]` | Vincula portas canônicas do host via socat no lado do host (o encaminhador aponta para a porta virtual estável do projeto). Desloca detentores de instâncias Coast com um aviso; gera erro em processos desconhecidos no host. Veja [Checkout](CHECKOUT.md). |
| `coast ssg uncheckout [--service <name> \| --all]` | Desmonta socats de porta canônica para este projeto. Não restaura automaticamente Coasts deslocados. |

### Diagnóstico

| Command | Summary |
|---------|---------|
| `coast ssg doctor` | Verificação somente leitura sobre permissões de bind-mount do host para serviços de imagens conhecidas e segredos SSG declarados, mas não extraídos. Emite achados `ok` / `warn` / `info`. Veja [Volumes -> coast ssg doctor](VOLUMES.md#coast-ssg-doctor). |

### Fixação de build

| Command | Summary |
|---------|---------|
| `coast ssg checkout-build <BUILD_ID> [--working-dir <dir>] [-f <file>]` | Fixa o SSG deste projeto a um `build_id` específico. `coast ssg run` e `coast build` usam a fixação em vez de `latest_build_id`. Veja [Building -> Locking a project to a specific build](BUILDING.md#locking-a-project-to-a-specific-build). |
| `coast ssg uncheckout-build [--working-dir <dir>] [-f <file>]` | Libera a fixação. Idempotente. |
| `coast ssg show-pin [--working-dir <dir>] [-f <file>]` | Mostra a fixação atual deste projeto, se houver. |

### Segredos nativos de SSG

| Command | Summary |
|---------|---------|
| `coast ssg secrets clear` | Remove toda entrada criptografada do keystore em `coast_image = "ssg:<project>"`. Idempotente. O único verbo que apaga segredos nativos de SSG -- `coast ssg rm` e `rm --with-data` deliberadamente os deixam intactos. Veja [Secrets](SECRETS.md). |

### Auxiliar de migração

| Command | Summary |
|---------|---------|
| `coast ssg import-host-volume <VOLUME> --service <name> --mount <path> [--apply] [-f <file>] [--working-dir <dir>] [--config '<toml>']` | Resolve o mountpoint de um volume nomeado Docker do host e emite (ou aplica) a entrada equivalente de bind-mount de SSG. Veja [Volumes -> coast ssg import-host-volume](VOLUMES.md#automating-the-recipe-coast-ssg-import-host-volume). |

## Códigos de saída

- `0` -- sucesso. Comandos como `doctor` retornam 0 mesmo quando encontram avisos; são ferramentas de diagnóstico, não barreiras.
- Não zero -- erro de validação, erro do Docker, inconsistência de estado ou recusa do gate de shadow remoto.

## Veja também

- [Building](BUILDING.md)
- [Lifecycle](LIFECYCLE.md)
- [Routing](ROUTING.md)
- [Volumes](VOLUMES.md)
- [Consuming](CONSUMING.md)
- [Secrets](SECRETS.md)
- [Checkout](CHECKOUT.md)

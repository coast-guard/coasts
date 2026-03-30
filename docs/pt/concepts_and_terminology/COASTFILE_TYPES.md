# Tipos de Coastfile

Um único projeto pode ter vários Coastfiles para diferentes casos de uso. Cada variante é chamada de "tipo". Os tipos permitem compor configurações que compartilham uma base comum, mas diferem em quais serviços são executados, como os volumes são tratados ou se os serviços iniciam automaticamente.

## Como os Tipos Funcionam

A convenção de nomenclatura é `Coastfile` para o padrão e `Coastfile.{type}` para variantes. O sufixo após o ponto se torna o nome do tipo:

- `Coastfile` -- tipo padrão
- `Coastfile.test` -- tipo de teste
- `Coastfile.snap` -- tipo de snapshot
- `Coastfile.light` -- tipo leve

Qualquer Coastfile pode ter uma extensão `.toml` opcional para realce de sintaxe no editor. O sufixo `.toml` é removido antes de derivar o tipo, então estes são pares equivalentes:

- `Coastfile.toml` = `Coastfile` (tipo padrão)
- `Coastfile.test.toml` = `Coastfile.test` (tipo de teste)
- `Coastfile.light.toml` = `Coastfile.light` (tipo leve)

**Regra de desempate:** se ambas as formas existirem (por exemplo, `Coastfile` e `Coastfile.toml`, ou `Coastfile.light` e `Coastfile.light.toml`), a variante `.toml` tem precedência.

**Nomes de tipo reservados:** `"default"` e `"toml"` não podem ser usados como nomes de tipo. `Coastfile.default` e `Coastfile.toml` (como um sufixo de tipo, significando um arquivo literalmente chamado `Coastfile.toml.toml`) são rejeitados.

Você cria e executa Coasts tipados com `--type`:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

Um Coastfile tipado herda de um pai por meio de `extends`. Tudo do pai é mesclado. O filho só precisa especificar o que substitui ou adiciona.

```toml
[coast]
extends = "Coastfile"
```

Isso evita duplicar toda a sua configuração para cada variante. O filho herda todas as [ports](PORTS.md), [secrets](SECRETS.md), [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), [assign strategies](ASSIGN.md), comandos de setup e configurações de [MCP](MCP_SERVERS.md) do pai. Qualquer coisa que o filho definir terá precedência sobre o pai.

## [unset]

Remove itens específicos herdados do pai pelo nome. Você pode remover `ports`, `shared_services`, `secrets` e `volumes`.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

É assim que uma variante de teste remove serviços compartilhados (para que bancos de dados sejam executados dentro do Coast com volumes isolados) e remove portas de que não precisa.

## [omit]

Remove completamente serviços do compose da build. Os serviços omitidos são removidos do arquivo compose e não são executados dentro do Coast de forma alguma.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

Use isso para excluir serviços que são irrelevantes para o propósito da variante. Uma variante de teste pode manter apenas o banco de dados, as migrações e o executor de testes.

## autostart

Controla se `docker compose up` é executado automaticamente quando o Coast inicia. O padrão é `true`.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

Defina `autostart = false` para variantes em que você deseja executar comandos específicos manualmente em vez de subir a stack completa. Isso é comum para executores de teste -- você cria o Coast e depois usa [`coast exec`](EXEC_AND_DOCKER.md) para executar suítes de teste individuais.

## Padrões Comuns

### Variante de teste

Um `Coastfile.test` que mantém apenas o que é necessário para executar testes:

```toml
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]

[omit]
services = ["redis", "backend", "mailhog", "web"]

[volumes.postgres_data]
strategy = "isolated"
service = "postgres"
mount = "/var/lib/postgresql/data"

[assign]
default = "none"
[assign.services]
test-runner = "rebuild"
migrations = "rebuild"
```

Cada Coast de teste recebe seu próprio banco de dados limpo. Nenhuma porta é exposta porque os testes se comunicam com os serviços pela rede interna do compose. `autostart = false` significa que você aciona as execuções de teste manualmente com `coast exec`.

### Variante de snapshot

Um `Coastfile.snap` que inicializa cada Coast com uma cópia dos volumes de banco de dados existentes no host:

```toml
[coast]
extends = "Coastfile"

[unset]
shared_services = ["postgres", "redis"]

[volumes.postgres_data]
strategy = "isolated"
snapshot_source = "my_project_postgres_data"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "isolated"
snapshot_source = "my_project_redis_data"
service = "redis"
mount = "/data"
```

Os serviços compartilhados são removidos para que os bancos de dados sejam executados dentro de cada Coast. `snapshot_source` inicializa os volumes isolados a partir de volumes existentes do host no momento da build. Após a criação, os dados de cada instância divergem de forma independente.

### Variante leve

Um `Coastfile.light` que reduz o projeto ao mínimo para um fluxo de trabalho específico -- talvez apenas um serviço de backend e seu banco de dados para iteração rápida.

## Pools de Build Independentes

Cada tipo tem seu próprio link simbólico `latest-{type}` e seu próprio pool de limpeza automática de 5 builds:

```bash
coast build              # atualiza latest, limpa builds padrão
coast build --type test  # atualiza latest-test, limpa builds de teste
coast build --type snap  # atualiza latest-snap, limpa builds snap
```

Construir um tipo `test` não afeta builds `default` ou `snap`. A limpeza é completamente independente por tipo.

## Executando Coasts Tipados

Instâncias criadas com `--type` são marcadas com seu tipo. Você pode ter instâncias de diferentes tipos em execução simultaneamente para o mesmo projeto:

```bash
coast run dev-1                    # tipo padrão
coast run test-1 --type test       # tipo de teste
coast run snapshot-1 --type snap   # tipo de snapshot

coast ls
# Todas as três aparecem, cada uma com seu próprio tipo, portas e estratégia de volume
```

É assim que você pode ter um ambiente completo de desenvolvimento em execução ao lado de executores de teste isolados e instâncias inicializadas por snapshot, tudo para o mesmo projeto, tudo ao mesmo tempo.

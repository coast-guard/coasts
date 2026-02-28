# Tipos de Coastfile

Um único projeto pode ter múltiplos Coastfiles para diferentes casos de uso. Cada variante é chamada de "tipo". Os tipos permitem compor configurações que compartilham uma base comum, mas diferem em quais serviços são executados, como os volumes são tratados ou se os serviços iniciam automaticamente.

## Como os Tipos Funcionam

A convenção de nomenclatura é `Coastfile` para o padrão e `Coastfile.{type}` para variantes. O sufixo após o ponto se torna o nome do tipo:

- `Coastfile` -- tipo padrão
- `Coastfile.test` -- tipo de teste
- `Coastfile.snap` -- tipo de snapshot
- `Coastfile.light` -- tipo leve

Você compila e executa Coasts tipados com `--type`:

```bash
coast build --type test
coast run test-1 --type test
coast exec test-1 -- go test ./...
```

## extends

Um Coastfile tipado herda de um pai via `extends`. Tudo do pai é mesclado. O filho só precisa especificar o que ele sobrescreve ou adiciona.

```toml
[coast]
extends = "Coastfile"
```

Isso evita duplicar toda a sua configuração para cada variante. O filho herda todas as configurações de [ports](PORTS.md), [secrets](SECRETS.md), [volumes](VOLUMES.md), [shared services](SHARED_SERVICES.md), [assign strategies](ASSIGN.md), comandos de setup e configurações de [MCP](MCP_SERVERS.md) do pai. Qualquer coisa que o filho definir tem precedência sobre o pai.

## [unset]

Remove itens específicos herdados do pai pelo nome. Você pode remover `ports`, `shared_services`, `secrets` e `volumes`.

```toml
[unset]
ports = ["web", "redis", "backend"]
shared_services = ["postgres", "redis"]
```

É assim que uma variante de teste remove serviços compartilhados (para que os bancos de dados rodem dentro do Coast com volumes isolados) e remove portas de que não precisa.

## [omit]

Remove serviços do compose inteiramente da build. Serviços omitidos são removidos do arquivo compose e não são executados dentro do Coast de forma alguma.

```toml
[omit]
services = ["redis", "backend", "mailhog", "web"]
```

Use isso para excluir serviços que são irrelevantes para o propósito da variante. Uma variante de teste pode manter apenas o banco de dados, as migrações e o executor de testes.

## autostart

Controla se `docker compose up` roda automaticamente quando o Coast inicia. O padrão é `true`.

```toml
[coast]
extends = "Coastfile"
autostart = false
```

Defina `autostart = false` para variantes em que você quer executar comandos específicos manualmente em vez de subir a stack completa. Isso é comum para executores de teste -- você cria o Coast e então usa [`coast exec`](EXEC_AND_DOCKER.md) para executar suítes de teste individuais.

## Padrões Comuns

### Variante de teste

Um `Coastfile.test` que mantém apenas o que é necessário para rodar testes:

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

Cada Coast de teste recebe seu próprio banco de dados limpo. Nenhuma porta é exposta porque os testes se comunicam com os serviços pela rede interna do compose. `autostart = false` significa que você dispara as execuções de teste manualmente com `coast exec`.

### Variante de snapshot

Um `Coastfile.snap` que prepara cada Coast com uma cópia dos volumes de banco de dados existentes do host:

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

Serviços compartilhados são removidos para que os bancos de dados rodem dentro de cada Coast. `snapshot_source` inicializa os volumes isolados a partir de volumes existentes do host no momento da build. Após a criação, os dados de cada instância divergem de forma independente.

### Variante leve

Um `Coastfile.light` que reduz o projeto ao mínimo para um fluxo de trabalho específico -- talvez apenas um serviço de backend e seu banco de dados para iteração rápida.

## Pools de Build Independentes

Cada tipo tem seu próprio symlink `latest-{type}` e seu próprio pool de auto-limpeza de 5 builds:

```bash
coast build              # atualiza latest, faz prune das builds padrão
coast build --type test  # atualiza latest-test, faz prune das builds de test
coast build --type snap  # atualiza latest-snap, faz prune das builds de snap
```

Construir um tipo `test` não afeta builds `default` nem `snap`. O prune é completamente independente por tipo.

## Executando Coasts Tipados

Instâncias criadas com `--type` são marcadas com seu tipo. Você pode ter instâncias de diferentes tipos rodando simultaneamente para o mesmo projeto:

```bash
coast run dev-1                    # tipo padrão
coast run test-1 --type test       # tipo de teste
coast run snapshot-1 --type snap   # tipo de snapshot

coast ls
# Todas as três aparecem, cada uma com seu próprio tipo, portas e estratégia de volume
```

É assim que você pode ter um ambiente completo de dev rodando ao lado de executores de teste isolados e instâncias inicializadas por snapshot, tudo para o mesmo projeto, tudo ao mesmo tempo.

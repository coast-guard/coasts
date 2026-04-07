# Portas

O Coast gerencia dois tipos de mapeamentos de portas para cada serviço em uma instância do Coast: portas canônicas e portas dinâmicas.

## Portas Canônicas

Estas são as portas em que seu projeto normalmente roda — aquelas no seu `docker-compose.yml` ou na configuração local de desenvolvimento. Por exemplo, `3000` para um servidor web, `5432` para o Postgres.

Apenas um Coast pode ter portas canônicas por vez. Aquele que estiver em [checkout](CHECKOUT.md) as recebe.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

Isso significa que seu navegador, clientes de API, ferramentas de banco de dados e suítes de teste funcionam exatamente como normalmente funcionariam — sem necessidade de alterar números de porta.

No Linux, portas canônicas abaixo de `1024` podem exigir configuração do host antes que [`coast checkout`](CHECKOUT.md) possa vinculá-las. Portas dinâmicas não têm essa restrição.

## Portas Dinâmicas

Cada Coast em execução sempre recebe seu próprio conjunto de portas dinâmicas em uma faixa alta (49152–65535). Elas são atribuídas automaticamente e estão sempre acessíveis, independentemente de qual Coast esteja em checkout.

```text
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681

coast ports dev-2

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       63104
#   db       5432       57220
```

As portas dinâmicas permitem que você dê uma olhada em qualquer Coast sem colocá-lo em checkout. Você pode abrir `localhost:63104` para acessar o servidor web do dev-2 enquanto o dev-1 está em checkout nas portas canônicas.

## Como Elas Funcionam Juntas

```text
┌──────────────────────────────────────────────────┐
│  Sua máquina                                     │
│                                                  │
│  Canônicas (apenas Coast em checkout):           │
│    localhost:3000 ──→ dev-1 web                  │
│    localhost:5432 ──→ dev-1 db                   │
│                                                  │
│  Dinâmicas (sempre disponíveis):                 │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

Alternar o [checkout](CHECKOUT.md) é instantâneo. O Coast encerra e recria encaminhadores leves do `socat`. Nenhum contêiner é reiniciado.

## Variáveis de Ambiente de Porta Dinâmica

O Coast injeta variáveis de ambiente em cada instância que expõem a porta dinâmica de cada serviço. O nome da variável é derivado da chave `[ports]`: `web` torna-se `WEB_DYNAMIC_PORT`, `backend-test` torna-se `BACKEND_TEST_DYNAMIC_PORT`.

Elas são úteis quando um serviço precisa conhecer sua porta externamente acessível, por exemplo para definir `AUTH_URL` para redirecionamentos de callback de autenticação. Veja [Variáveis de Ambiente de Porta Dinâmica](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) para a referência completa.

## Portas e Coasts Remotos

Para [coasts remotos](REMOTES.md), as portas passam por uma camada adicional de túnel SSH. Cada porta dinâmica local é encaminhada via `ssh -L` para uma porta dinâmica remota correspondente, que por sua vez é mapeada para a porta canônica dentro do contêiner DinD remoto. Isso é transparente -- `coast ports` e `coast checkout` funcionam de forma idêntica para instâncias locais e remotas.

## Veja Também

- [Porta Primária & DNS](PRIMARY_PORT_AND_DNS.md) - links rápidos, roteamento por subdomínio e modelos de URL
- [Variáveis de Ambiente de Porta Dinâmica](DYNAMIC_PORT_ENVIRONMENT_VARIABLES.md) - usando `WEB_DYNAMIC_PORT` e variáveis relacionadas em comandos de serviço
- [Remotos](REMOTES.md) - como o encaminhamento de portas funciona para coasts remotos

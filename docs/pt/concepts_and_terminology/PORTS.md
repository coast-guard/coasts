# Portas

O Coast gerencia dois tipos de mapeamentos de portas para cada serviço em uma instância do Coast: portas canônicas e portas dinâmicas.

## Portas Canônicas

Estas são as portas em que seu projeto normalmente roda — as do seu `docker-compose.yml` ou da configuração local de desenvolvimento. Por exemplo, `3000` para um servidor web, `5432` para o Postgres.

Apenas um Coast pode ter as portas canônicas por vez. O Coast que estiver com [checkout](CHECKOUT.md) recebe essas portas.

```text
coast checkout dev-1

localhost:3000  ──→  dev-1
localhost:5432  ──→  dev-1
```

Isso significa que seu navegador, clientes de API, ferramentas de banco de dados e suítes de teste funcionam exatamente como normalmente funcionariam — sem necessidade de alterar números de porta.

## Portas Dinâmicas

Todo Coast em execução sempre recebe seu próprio conjunto de portas dinâmicas em uma faixa alta (49152–65535). Elas são atribuídas automaticamente e estão sempre acessíveis, independentemente de qual Coast está com checkout.

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

Portas dinâmicas permitem que você dê uma olhada em qualquer Coast sem fazer checkout nele. Você pode abrir `localhost:63104` para acessar o servidor web do dev-2 enquanto o dev-1 está com checkout nas portas canônicas.

## Como Elas Funcionam Juntas

```text
┌──────────────────────────────────────────────────┐
│  Sua máquina                                     │
│                                                  │
│  Canônicas (apenas o Coast com checkout):        │
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

Alternar o [checkout](CHECKOUT.md) é instantâneo — o Coast encerra e recria encaminhadores `socat` leves. Nenhum container é reiniciado.

Veja também [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) para links rápidos, roteamento por subdomínio e modelos de URL.

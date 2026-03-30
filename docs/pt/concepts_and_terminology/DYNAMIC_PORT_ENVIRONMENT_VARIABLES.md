# Variáveis de Ambiente de Porta Dinâmica

Toda instância do Coast recebe um conjunto de variáveis de ambiente que expõem a [porta dinâmica](PORTS.md) atribuída a cada serviço. Essas variáveis estão disponíveis tanto dentro de serviços bare quanto de contêineres compose, e permitem que sua aplicação descubra em tempo de execução sua porta acessível externamente.

## Convenção de Nomenclatura

O Coast deriva o nome da variável a partir do nome lógico do serviço na sua seção `[ports]`:

1. Converter para maiúsculas
2. Substituir caracteres não alfanuméricos por sublinhados
3. Acrescentar `_DYNAMIC_PORT`

```text
[ports] key          Variável de ambiente
─────────────        ────────────────────────────
web             →    WEB_DYNAMIC_PORT
postgres        →    POSTGRES_DYNAMIC_PORT
backend-test    →    BACKEND_TEST_DYNAMIC_PORT
svc.v2          →    SVC_V2_DYNAMIC_PORT
```

Se o nome do serviço começar com um dígito, o Coast prefixa a variável com um sublinhado (por exemplo, `9svc` torna-se `_9SVC_DYNAMIC_PORT`). Um nome vazio recorre a `SERVICE_DYNAMIC_PORT`.

## Exemplo

Dado este Coastfile:

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

Toda instância do Coast criada a partir deste build terá três variáveis de ambiente adicionais:

```text
WEB_DYNAMIC_PORT=62217
API_DYNAMIC_PORT=55681
POSTGRES_DYNAMIC_PORT=56905
```

Os números reais das portas são atribuídos no momento de `coast run` e diferem por instância.

## Quando Usá-las

O caso de uso mais comum é configurar serviços que incorporam sua própria URL nas respostas: callbacks de autenticação, URIs de redirecionamento OAuth, origens CORS ou URLs de webhook. Esses serviços precisam saber a porta que os clientes externos usam, não a porta interna na qual escutam.

Por exemplo, uma aplicação Next.js usando NextAuth precisa que `AUTH_URL` seja definida com o endereço acessível externamente. Dentro do Coast, o Next.js sempre escuta na porta 3000, mas a porta no host é dinâmica:

```toml
[services.web]
command = "cd /workspace && AUTH_URL=http://localhost:${WEB_DYNAMIC_PORT:-3000} yarn dev:web"
port = 3000
```

O fallback `:-3000` significa que o comando também funciona fora do Coast, onde `WEB_DYNAMIC_PORT` não está definida.

## Precedência

Se uma variável de ambiente com o mesmo nome já existir no contêiner do Coast (definida via secrets, inject ou compose environment), o Coast não a sobrescreve. O valor existente tem precedência.

## Disponibilidade

As variáveis de porta dinâmica são injetadas no ambiente do contêiner do Coast na inicialização. Elas estão disponíveis para:

- Comandos `install` de serviço bare
- Processos `command` de serviço bare
- Contêineres de serviço compose (via o ambiente do contêiner)
- Comandos executados por meio de `coast exec`

Os valores não mudam durante o tempo de vida da instância. Se você parar e iniciar a instância, ela manterá as mesmas portas dinâmicas.

## Veja Também

- [Ports](PORTS.md) - portas canônicas vs dinâmicas e como o checkout alterna entre elas
- [Primary Port & DNS](PRIMARY_PORT_AND_DNS.md) - roteamento por subdomínio e isolamento de cookies entre instâncias

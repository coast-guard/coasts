# Checkout

Checkout controla qual instância do Coast possui as [portas canônicas](PORTS.md) do seu projeto. Quando você faz checkout de um Coast, `localhost:3000`, `localhost:5432` e todas as outras portas canônicas passam a mapear diretamente para essa instância.

```bash
coast checkout dev-1
```

```text
Antes do checkout:
  localhost:3000  ──→  (nada)
  localhost:5432  ──→  (nada)

Depois do checkout:
  localhost:3000  ──→  dev-1 web
  localhost:5432  ──→  dev-1 db
```

Trocar o checkout é instantâneo — o Coast encerra e recria encaminhadores `socat` leves. Nenhum contêiner é reiniciado.

```bash
coast checkout dev-2   # troca instantânea

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Observação sobre Linux

As portas dinâmicas sempre funcionam no Linux sem privilégios especiais.

As portas canônicas abaixo de `1024` são diferentes. Se o seu Coastfile declarar portas como `80` ou `443`, o Linux pode impedir que `coast checkout` faça o bind delas até que você configure o host. As correções mais comuns são:

- aumentar `net.ipv4.ip_unprivileged_port_start`
- conceder capacidade de bind ao binário ou processo de encaminhamento

O Coast informa isso explicitamente quando o host nega o bind.

No WSL, o Coast usa bridges de checkout publicadas pelo Docker para que navegadores e ferramentas do Windows possam alcançar as portas canônicas em checkout através de `127.0.0.1`, de forma semelhante a fluxos de trabalho do Docker Desktop como o Sail.

Para projetos HTTPS locais que usam Caddy, o Coast reutiliza uma CA local do Caddy por instalação do Coast. Depois que você confia nessa raiz uma vez, workspaces recriados sob a mesma instalação continuam usando-a.

O certificado raiz fica em:

- `~/.coast/caddy/pki/authorities/local/root.crt` para a instalação normal
- `~/.coast-dev/caddy/pki/authorities/local/root.crt` para `coast-dev`

Eles são intencionalmente separados, então confiar em `coast-dev` não faz com que uma instalação normal do `coast` também seja confiável, e vice-versa.

Para inspecionar ou exportar o certificado raiz da instalação ativa:

```bash
coast cert info
coast cert path
coast cert fingerprint
coast cert export --to ~/Downloads/coast-root.crt
```

O Coast deixa a instalação da confiança por sua conta. Exporte o certificado e, em seguida, importe-o para o armazenamento de confiança do seu sistema operacional ou navegador, conforme necessário.

## Você Precisa Fazer Checkout?

Não necessariamente. Todo Coast em execução sempre tem suas próprias portas dinâmicas, e você pode acessar qualquer Coast por essas portas a qualquer momento sem fazer checkout de nada.

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Você pode abrir `localhost:62217` no seu navegador para acessar o servidor web do dev-1 sem fazer checkout. Isso é perfeitamente adequado para muitos fluxos de trabalho, e você pode executar quantos Coasts quiser sem nunca usar `coast checkout`.

## Quando o Checkout É Útil

Há situações em que portas dinâmicas não são suficientes e você precisa de portas canônicas:

- **Aplicações cliente com portas canônicas definidas no código.** Se você tem um cliente rodando fora do Coast — um servidor de desenvolvimento frontend no seu host, um aplicativo móvel no seu telefone ou um aplicativo desktop — que espera `localhost:3000` ou `localhost:8080`, alterar números de porta em todos os lugares é impraticável. Fazer checkout do Coast fornece as portas reais sem mudar nenhuma configuração.

- **Webhooks e URLs de callback.** Serviços como Stripe, GitHub ou provedores OAuth enviam callbacks para uma URL que você registrou — normalmente algo como `https://your-ngrok-tunnel.io` que encaminha para `localhost:3000`. Se você mudar para uma porta dinâmica, os callbacks deixam de chegar. Fazer checkout garante que a porta canônica esteja ativa para o Coast que você está testando.

- **Ferramentas de banco de dados, depuradores e integrações de IDE.** Muitos clientes GUI (pgAdmin, DataGrip, TablePlus), depuradores e configurações de execução da IDE salvam perfis de conexão com uma porta específica. O checkout permite que você mantenha seus perfis salvos e apenas troque qual Coast está por trás deles — sem reconfigurar seu alvo de anexação do depurador ou conexão com o banco de dados toda vez que você muda de contexto.

## Liberando o Checkout

Se você quiser liberar as portas canônicas sem fazer checkout de um Coast diferente:

```bash
coast checkout --none
```

Depois disso, nenhum Coast possui as portas canônicas. Todos os Coasts continuam acessíveis por meio de suas portas dinâmicas.

## Apenas Um por Vez

Exatamente um Coast pode estar em checkout por vez. Se `dev-1` estiver em checkout e você executar `coast checkout dev-2`, as portas canônicas trocam instantaneamente para `dev-2`. Não há intervalo — os encaminhadores antigos são encerrados e novos são iniciados na mesma operação.

```text
┌──────────────────────────────────────────────────┐
│  Sua máquina                                     │
│                                                  │
│  Canônicas (apenas Coast em checkout):           │
│    localhost:3000 ──→ dev-2 web                  │
│    localhost:5432 ──→ dev-2 db                   │
│                                                  │
│  Dinâmicas (sempre disponíveis):                 │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

As portas dinâmicas não são afetadas pelo checkout. A única coisa que muda é para onde as portas canônicas apontam.

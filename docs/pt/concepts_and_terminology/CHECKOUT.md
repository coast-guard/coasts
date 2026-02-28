# Checkout

O checkout controla qual instância do Coast é dona das [portas canônicas](PORTS.md) do seu projeto. Quando você faz checkout de um Coast, `localhost:3000`, `localhost:5432` e todas as outras portas canônicas mapeiam diretamente para essa instância.

```bash
coast checkout dev-1
```

```text
Before checkout:
  localhost:3000  ──→  (nothing)
  localhost:5432  ──→  (nothing)

After checkout:
  localhost:3000  ──→  dev-1 web
  localhost:5432  ──→  dev-1 db
```

Trocar o checkout é instantâneo — o Coast encerra e recria encaminhadores leves do `socat`. Nenhum contêiner é reiniciado.

```bash
coast checkout dev-2   # instant swap

# localhost:3000  ──→  dev-2 web
# localhost:5432  ──→  dev-2 db
```

## Você Precisa Fazer Checkout?

Não necessariamente. Todo Coast em execução sempre tem suas próprias portas dinâmicas, e você pode acessar qualquer Coast por meio dessas portas a qualquer momento sem fazer checkout de nada.

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Você pode abrir `localhost:62217` no seu navegador para acessar o servidor web do dev-1 sem fazer checkout. Isso é perfeitamente aceitável para muitos fluxos de trabalho, e você pode executar quantos Coasts quiser sem nunca usar `coast checkout`.

## Quando o Checkout É Útil

Há situações em que portas dinâmicas não são suficientes e você precisa de portas canônicas:

- **Aplicações clientes com portas canônicas codificadas.** Se você tem um cliente rodando fora do Coast — um servidor de desenvolvimento de frontend no seu host, um app móvel no seu telefone ou um app desktop — que espera `localhost:3000` ou `localhost:8080`, mudar números de porta em todo lugar é impraticável. Fazer checkout do Coast fornece as portas reais sem alterar nenhuma configuração.

- **Webhooks e URLs de callback.** Serviços como Stripe, GitHub ou provedores OAuth enviam callbacks para uma URL que você registrou — geralmente algo como `https://your-ngrok-tunnel.io` que encaminha para `localhost:3000`. Se você trocar para uma porta dinâmica, os callbacks param de chegar. O checkout garante que a porta canônica esteja ativa para o Coast que você está testando.

- **Ferramentas de banco de dados, depuradores e integrações de IDE.** Muitos clientes GUI (pgAdmin, DataGrip, TablePlus), depuradores e configurações de execução de IDE salvam perfis de conexão com uma porta específica. O checkout permite manter seus perfis salvos e apenas trocar qual Coast está por trás deles — sem reconfigurar o alvo de attach do depurador ou a conexão do banco de dados toda vez que você muda de contexto.

## Liberando o Checkout

Se você quiser liberar as portas canônicas sem fazer checkout de um Coast diferente:

```bash
coast checkout --none
```

Depois disso, nenhum Coast possui as portas canônicas. Todos os Coasts permanecem acessíveis por meio de suas portas dinâmicas.

## Apenas Um por Vez

Exatamente um Coast pode estar em checkout por vez. Se `dev-1` estiver em checkout e você executar `coast checkout dev-2`, as portas canônicas trocam instantaneamente para `dev-2`. Não há intervalo — os encaminhadores antigos são encerrados e novos são criados na mesma operação.

```text
┌──────────────────────────────────────────────────┐
│  Your machine                                    │
│                                                  │
│  Canonical (checked-out Coast only):             │
│    localhost:3000 ──→ dev-2 web                  │
│    localhost:5432 ──→ dev-2 db                   │
│                                                  │
│  Dynamic (always available):                     │
│    localhost:62217 ──→ dev-1 web                 │
│    localhost:55681 ──→ dev-1 db                  │
│    localhost:63104 ──→ dev-2 web                 │
│    localhost:57220 ──→ dev-2 db                  │
└──────────────────────────────────────────────────┘
```

As portas dinâmicas não são afetadas pelo checkout. A única coisa que muda é para onde as portas canônicas apontam.

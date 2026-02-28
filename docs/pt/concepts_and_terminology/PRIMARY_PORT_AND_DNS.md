# Porta Primária e DNS

A porta primária é um recurso opcional de conveniência que cria um link rápido para um dos seus serviços — normalmente seu frontend web. Ela aparece como um selo clicável no Coastguard e como uma entrada marcada com estrela em `coast ports`. Ela não altera como as portas funcionam; apenas escolhe uma para destacar.

## Definindo a Porta Primária

Adicione `primary_port` à seção `[coast]` do seu Coastfile, referenciando uma chave de [`[ports]`](PORTS.md):

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
backend = 8080
```

Se o seu projeto tiver apenas uma porta, o Coast a detecta automaticamente como a primária — você não precisa defini-la explicitamente.

Você também pode alternar a primária na aba Ports do Coastguard clicando no ícone de estrela ao lado de qualquer serviço, ou via CLI com `coast ports set-primary`. A configuração é por build, então todas as instâncias criadas a partir do mesmo build compartilham a mesma primária.

## O que Isso Habilita

```text
coast ports dev-1

  SERVICE    CANONICAL  DYNAMIC
  ★ web      3000       62217
    backend  8080       63889
```

O serviço com estrela é o seu primário. No Coastguard, ele aparece como um selo clicável ao lado do nome da instância — um clique abre seu app no navegador.

Isso é particularmente útil para:

- **Agentes no host** — dê ao seu agente de IA uma única URL para verificar as mudanças. Em vez de dizer "abra localhost:62217", a URL da porta primária fica disponível programaticamente a partir de `coast ls` e da API do daemon.
- **MCPs de navegador** — se o seu agente usa um MCP de navegador para verificar mudanças de UI, a URL da porta primária é o alvo canônico para apontá-lo.
- **Iteração rápida** — acesso com um clique ao serviço que você consulta com mais frequência.

A porta primária é totalmente opcional. Tudo funciona sem ela — é um recurso de qualidade de vida para navegação mais rápida.

## Roteamento por Subdomínio

Quando você executa múltiplas instâncias do Coast com bancos de dados isolados, todas compartilham `localhost` no navegador. Isso significa que cookies definidos por `localhost:62217` (dev-1) ficam visíveis para `localhost:63104` (dev-2). Se o seu app usa cookies de sessão, fazer login em uma instância pode interferir em outra.

O roteamento por subdomínio resolve isso dando a cada instância sua própria origem:

```text
Sem roteamento por subdomínio:
  dev-1 web  →  http://localhost:62217
  dev-2 web  →  http://localhost:63104
  (cookies compartilhados — ambos são "localhost")

Com roteamento por subdomínio:
  dev-1 web  →  http://dev-1.localhost:62217
  dev-2 web  →  http://dev-2.localhost:63104
  (cookies isolados — subdomínios diferentes)
```

Habilite por projeto na aba Ports do Coastguard (alternância na parte inferior da página) ou via API de configurações do daemon.

### Compensação: CORS

A desvantagem é que seu aplicativo pode precisar de ajustes de CORS. Se o seu frontend em `dev-1.localhost:3000` faz requisições de API para `dev-1.localhost:8080`, o navegador trata isso como cross-origin porque a porta difere. A maioria dos servidores de desenvolvimento já lida com isso, mas se você vir erros de CORS após habilitar o roteamento por subdomínio, verifique a configuração de origens permitidas do seu aplicativo.

## Modelos de URL

Cada serviço tem um modelo de URL que controla como seus links são gerados. O padrão é:

```text
http://localhost:<port>
```

O placeholder `<port>` é substituído pelo número de porta real — a porta canônica quando a instância está em [checkout](CHECKOUT.md), ou a porta dinâmica caso contrário. Quando o roteamento por subdomínio está habilitado, `localhost:` é substituído por `{instance}.localhost:`.

Você pode personalizar modelos por serviço na aba Ports do Coastguard (ícone de lápis ao lado de cada serviço). Isso é útil se seu servidor de desenvolvimento usa HTTPS, um hostname personalizado ou um esquema de URL não padrão:

```text
https://my-service.localhost:<port>
```

Os modelos são armazenados nas configurações do daemon e persistem entre reinicializações.

## Configuração de DNS

A maioria dos navegadores resolve `*.localhost` para `127.0.0.1` imediatamente, então o roteamento por subdomínio funciona sem qualquer configuração de DNS.

Se você precisar de resolução de domínio personalizada (ex.: `*.localcoast`), o Coast inclui um servidor DNS embutido. Configure uma vez:

```bash
coast dns setup    # escreve /etc/resolver/localcoast (requer sudo)
coast dns status   # verifique se o DNS está configurado
coast dns remove   # remove a entrada do resolver
```

Isso é opcional e só é necessário se `*.localhost` não funcionar no seu navegador ou se você quiser um TLD personalizado.

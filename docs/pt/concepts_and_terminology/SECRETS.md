# Segredos e Extratores

Segredos são valores extraídos da sua máquina host e injetados em contêineres Coast como variáveis de ambiente ou arquivos. O Coast extrai segredos no momento do build, criptografa-os em repouso em um keystore local e os injeta quando uma instância Coast é criada.

## Tipos de Injeção

Todo segredo tem um alvo `inject` que controla como ele é entregue no contêiner Coast:

- `env:VAR_NAME` — injetado como uma variável de ambiente.
- `file:/path/in/container` — montado como um arquivo dentro do contêiner.

```toml
[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.credentials]
extractor = "file"
path = "~/.config/my-app/credentials.json"
inject = "file:/run/secrets/credentials.json"
```

## Extratores Integrados

### env

Lê uma variável de ambiente do host. Este é o extrator mais comum e mais simples. Se você já tem segredos como variáveis de ambiente no seu host — a partir de arquivos `.env`, `direnv`, perfis do shell ou qualquer outra fonte — basta encaminhá-los para o Coast.

```toml
[secrets.db_password]
extractor = "env"
var = "DB_PASSWORD"
inject = "env:DATABASE_PASSWORD"
```

A maioria dos projetos consegue se virar apenas com o extrator `env`.

### file

Lê um arquivo do sistema de arquivos do host. Suporta expansão de `~` para caminhos do diretório home. Bom para chaves SSH, certificados TLS e arquivos JSON de credenciais.

```toml
[secrets.ssh_key]
extractor = "file"
path = "~/.ssh/id_ed25519"
inject = "file:/run/secrets/ssh_key"
```

### command

Executa um comando de shell e captura o stdout como o valor do segredo. O comando é executado via `sh -c`, então pipes, redirecionamentos e expansão de variáveis funcionam. Isso é útil para obter segredos do 1Password CLI, HashiCorp Vault ou qualquer fonte dinâmica.

```toml
[secrets.op_token]
extractor = "command"
run = "op read 'op://vault/db/password'"
inject = "env:DATABASE_PASSWORD"
```

Você também pode usar `command` para transformar ou extrair campos específicos de arquivos de configuração locais:

```toml
[secrets.claude_config]
extractor = "command"
run = 'python3 -c "import json; print(json.dumps({\"key\": \"value\"}))"'
inject = "file:/root/.claude.json"
```

### keychain

Alias para `macos-keychain`. Lê um item de senha genérica do Chaveiro do macOS.

```toml
[secrets.claude_credentials]
extractor = "keychain"
service = "Claude Code-credentials"
inject = "file:/root/.claude/.credentials.json"
```

O extrator de keychain frequentemente é desnecessário. Se você puder obter o mesmo valor por meio de uma variável de ambiente ou de um arquivo, prefira essas abordagens mais simples. A extração do Keychain é útil quando o segredo só existe no Chaveiro do macOS e não é facilmente exportado — por exemplo, credenciais específicas de aplicativo armazenadas por ferramentas de terceiros que escrevem diretamente no Keychain.

O parâmetro `account` é opcional e, por padrão, usa seu nome de usuário do macOS.

Este extrator está disponível apenas no macOS. Referenciá-lo em outras plataformas produz um erro claro no momento do build.

## Extratores Personalizados

Se nenhum dos extratores integrados se encaixar no seu fluxo de trabalho, o Coast recorre a procurar um executável chamado `coast-extractor-{name}` no seu PATH. O executável recebe os parâmetros do extrator como JSON no stdin e deve escrever o valor do segredo no stdout.

```toml
[secrets.vault_token]
extractor = "vault"
path = "secret/data/token"
inject = "env:VAULT_TOKEN"
```

O Coast irá invocar `coast-extractor-vault`, passando `{"path": "secret/data/token"}` no stdin. Código de saída 0 significa sucesso; diferente de zero significa falha (stderr é incluído na mensagem de erro).

## Injeção de Não-segredos

A seção `[inject]` encaminha variáveis de ambiente e arquivos do host para o Coast sem tratá-los como segredos. Esses valores não são criptografados — eles são passados diretamente.

```toml
[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.gitconfig", "~/.npmrc"]
```

Use `[inject]` para configuração que não é sensível. Use `[secrets]` para qualquer coisa que deva ser criptografada em repouso.

## Segredos Não São Armazenados no Build

Os segredos são extraídos no momento do build, mas não são incorporados ao artefato de build do coast. Eles são injetados quando uma instância Coast é criada com `coast run`. Isso significa que você pode compartilhar artefatos de build sem expor segredos.

Os segredos podem ser reinjetados em tempo de execução sem rebuild. Na UI do [Coastguard](COASTGUARD.md), use a ação **Re-run Secrets** na aba Secrets. A partir do CLI, use [`coast build --refresh`](BUILDS.md) para reextrair e atualizar segredos.

## TTL e Reextração

Segredos podem ter um campo `ttl` (time-to-live) opcional. Quando um segredo expira, `coast build --refresh` irá reextraí-lo da fonte.

```toml
[secrets.short_lived_token]
extractor = "command"
run = "generate-token --ttl 1h"
inject = "env:AUTH_TOKEN"
ttl = "1h"
```

## Criptografia em Repouso

Todos os segredos extraídos são criptografados com AES-256-GCM em um keystore local. A chave de criptografia é armazenada no Chaveiro do macOS no macOS, ou em um arquivo com permissões 0600 no Linux.

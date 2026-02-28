# Serviços Bare

> **Nota:** Serviços bare são executados diretamente dentro do contêiner Coast como processos simples — eles não são conteinerizados. Se seus serviços já estão dockerizados, use `compose` em vez disso. Serviços bare são mais adequados para configurações simples em que você quer evitar a sobrecarga de escrever um Dockerfile e um docker-compose.yml.

As seções `[services.*]` definem processos que o Coast executa diretamente dentro do contêiner DinD, sem Docker Compose. Esta é uma alternativa ao uso de um arquivo `compose` — você não pode usar ambos no mesmo Coastfile.

Serviços bare são supervisionados pelo Coast com captura de logs e políticas de reinício opcionais. Para um contexto mais aprofundado sobre como serviços bare funcionam, suas limitações e quando migrar para compose, veja [Bare Services](../concepts_and_terminology/BARE_SERVICES.md).

## Definindo um serviço

Cada serviço é uma seção TOML nomeada sob `[services]`. O campo `command` é obrigatório.

```toml
[services.web]
command = "node server.js"
port = 3000
```

### `command` (obrigatório)

O comando do shell a ser executado. Não deve estar vazio nem conter apenas espaços em branco.

```toml
[services.web]
command = "npx next dev --turbopack --port 3000 --hostname 0.0.0.0"
```

### `port`

A porta em que o serviço escuta. Usada para verificação de saúde e integração de encaminhamento de portas. Deve ser diferente de zero se especificada.

```toml
[services.web]
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

### `restart`

Política de reinício se o processo encerrar. O padrão é `"no"`.

- `"no"` — não reiniciar
- `"on-failure"` — reiniciar apenas se o processo encerrar com um código diferente de zero
- `"always"` — sempre reiniciar

```toml
[services.web]
command = "node server.js"
port = 3000
restart = "on-failure"
```

### `install`

Comandos a serem executados antes de iniciar o serviço (por exemplo, instalar dependências). Aceita uma única string ou um array de strings.

```toml
[services.web]
install = "npm install"
command = "npx next dev --port 3000 --hostname 0.0.0.0"
port = 3000
```

```toml
[services.web]
install = ["npm install", "npm run build"]
command = "npm start"
port = 3000
```

## Exclusão mútua com compose

Um Coastfile não pode definir tanto `compose` quanto `[services]`. Se você tiver um campo `compose` em `[coast]`, adicionar qualquer seção `[services.*]` é um erro. Escolha uma abordagem por Coastfile.

Se você precisar de alguns serviços conteinerizados via compose e alguns rodando como bare, use compose para todos eles — veja [a orientação de migração em Bare Services](../concepts_and_terminology/BARE_SERVICES.md) sobre como migrar de serviços bare para compose.

## Exemplos

### Aplicativo Next.js de serviço único

```toml
[coast]
name = "my-frontend"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "npx next dev --turbopack --port 3002 --hostname 0.0.0.0"
port = 3002
restart = "on-failure"

[ports]
web = 3002
```

### Servidor web com worker em segundo plano

```toml
[coast]
name = "my-app"

[coast.setup]
packages = ["nodejs", "npm"]

[services.web]
install = "npm install"
command = "node server.js"
port = 3000
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
web = 3000
```

### Serviço Python com instalação em várias etapas

```toml
[coast]
name = "ml-service"

[coast.setup]
packages = ["python3", "py3-pip"]

[services.api]
install = ["pip install -r requirements.txt", "python manage.py migrate"]
command = "python manage.py runserver 0.0.0.0:8000"
port = 8000
restart = "on-failure"

[ports]
api = 8000
```

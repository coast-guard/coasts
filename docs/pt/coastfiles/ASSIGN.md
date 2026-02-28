# Atribuir

A seção `[assign]` controla o que acontece com os serviços dentro de uma instância do Coast quando você troca de branches com `coast assign`. Cada serviço pode ser configurado com uma estratégia diferente dependendo se ele precisa de um rebuild completo, um restart, um hot-reload ou nada.

Para entender como `coast assign` e `coast unassign` funcionam em tempo de execução, veja [Assign](../concepts_and_terminology/ASSIGN.md).

## `[assign]`

### `default`

A ação padrão aplicada a todos os serviços na troca de branch. O padrão é `"restart"` se a seção `[assign]` inteira for omitida.

- **`"none"`** — não faz nada. O serviço continua em execução como está. Bom para bancos de dados e caches que não dependem de código.
- **`"hot"`** — o código já está montado ao vivo via o [filesystem](../concepts_and_terminology/FILESYSTEM.md), então o serviço aplica as mudanças automaticamente (ex.: via um file watcher ou hot-reload). Não é necessário reiniciar o container.
- **`"restart"`** — reinicia o container do serviço. Use quando o serviço lê o código na inicialização, mas não precisa de um rebuild completo da imagem.
- **`"rebuild"`** — faz rebuild da imagem Docker do serviço e reinicia. Necessário quando o código é incorporado na imagem via `COPY` ou `ADD` no Dockerfile.

```toml
[assign]
default = "none"
```

### `[assign.services]`

Overrides por serviço. Cada chave é um nome de serviço do compose, e o valor é uma das quatro ações acima.

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

Isso permite deixar bancos de dados e caches intocados (`"none"` via o default) enquanto faz rebuild ou restart apenas dos serviços que dependem do código que mudou.

### `[assign.rebuild_triggers]`

Padrões de arquivo que forçam um rebuild para serviços específicos, mesmo que a ação padrão deles seja algo mais leve. Cada chave é um nome de serviço, e o valor é uma lista de caminhos de arquivo ou padrões.

```toml
[assign]
default = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json", "package-lock.json"]
```

### `exclude_paths`

Uma lista de caminhos a excluir da sincronização do worktree durante `coast assign`. Útil em monorepos grandes onde certos diretórios são irrelevantes para os serviços em execução no Coast e, caso contrário, deixariam a operação de assign mais lenta.

```toml
[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension", "apps/ide-extension"]

[assign.services]
backend = "hot"
web = "hot"
```

## Exemplos

### Fazer rebuild do app, deixar todo o resto como está

Quando o seu serviço de app incorpora código na imagem Docker, mas seus bancos de dados são independentes de mudanças de código:

```toml
[assign]
default = "none"

[assign.services]
app = "rebuild"
```

### Hot-reload do frontend e backend

Quando ambos os serviços usam file watchers (ex.: servidor dev do Next.js, Go air, nodemon) e o código está montado ao vivo:

```toml
[assign]
default = "none"

[assign.services]
backend = "hot"
web = "hot"
```

### Rebuild por serviço com triggers

O serviço de API normalmente apenas reinicia, mas se `Dockerfile` ou `package.json` mudou, ele faz rebuild:

```toml
[assign]
default = "none"

[assign.services]
api = "restart"
worker = "restart"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
```

### Rebuild completo para tudo

Quando todos os serviços incorporam código nas suas imagens:

```toml
[assign]
default = "rebuild"
```

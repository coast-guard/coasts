# Primeiros passos com o Coasts

Se você ainda não fez isso, conclua primeiro a instalação e os requisitos abaixo. Em seguida, este guia mostra como usar o Coast em um projeto.

## Instalando

- `brew install coast`
- `coast daemon install`

*Se você decidir não executar `coast daemon install`, você será responsável por iniciar o daemon manualmente com `coast daemon start` toda vez, sem exceção.*

## Requisitos

- macOS
- Docker Desktop
- Um projeto usando Git
- Node.js
- `socat` *(instalado com `brew install coast` como uma dependência `depends_on` do Homebrew)*

```text
Nota sobre Linux: Ainda não testamos o Coasts no Linux, mas o suporte a Linux está planejado.
Você pode tentar executar o Coasts no Linux hoje, mas não oferecemos garantias de que ele funcionará corretamente.
```

## Configurando o Coasts em um projeto

Adicione um Coastfile à raiz do seu projeto. Certifique-se de não estar em um worktree ao instalar.

```text
my-project/
├── Coastfile              <-- isto é o que o Coast lê
├── docker-compose.yml
├── Dockerfile
├── src/
│   └── ...
└── ...
```

O `Coastfile` aponta para seus recursos de desenvolvimento local existentes e adiciona configuração específica do Coasts — veja a [documentação de Coastfiles](coastfiles/README.md) para o esquema completo:

```toml
[coast]
name = "my-project"
compose = "./docker-compose.yml"

[ports]
web = 3000
db = 5432
```

Um Coastfile é um arquivo TOML leve que *normalmente* aponta para o seu `docker-compose.yml` existente (ele também funciona com configurações de dev local sem contêiner) e descreve as modificações necessárias para executar seu projeto em paralelo — mapeamentos de porta, estratégias de volume e segredos. Coloque-o na raiz do seu projeto.

A forma mais rápida de criar um Coastfile para o seu projeto é deixar seu agente de codificação fazer isso.

A CLI do Coasts vem com um prompt embutido que ensina a qualquer agente de IA todo o esquema do Coastfile e a CLI. Você pode vê-lo aqui: [installation_prompt.txt](installation_prompt.txt)

Passe-o diretamente para seu agente, ou copie o [prompt de instalação](installation_prompt.txt) e cole no chat do seu agente:

```bash-emphasis
# Claude Code
claude -p "$(coast installation-prompt)"

# Codex
codex "$(coast installation-prompt)"

# Cursor (from terminal)
cursor --chat "$(coast installation-prompt)"
```

O prompt cobre o formato TOML do Coastfile, estratégias de volume, injeção de segredos e todos os comandos relevantes da CLI. Seu agente analisará seu projeto e gerará um Coastfile.

## Seu primeiro Coast

Antes de iniciar seu primeiro Coast, derrube qualquer ambiente de desenvolvimento em execução. Se você estiver usando Docker Compose, execute `docker-compose down`. Se você tiver servidores de dev locais em execução, pare-os. O Coasts gerencia suas próprias portas e entrará em conflito com qualquer coisa que já esteja escutando.

Quando seu Coastfile estiver pronto:

```bash
coast build
coast run dev-1
```

Verifique se sua instância está em execução:

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -             ~/dev/my-project
```

Veja onde seus serviços estão escutando:

```bash
coast ports dev-1

# SERVICE    CANONICAL  DYNAMIC
# ★ web      3000       62217
#   db       5432       55681
```

Cada instância recebe seu próprio conjunto de portas dinâmicas para que múltiplas instâncias possam rodar lado a lado. Para mapear uma instância de volta às portas canônicas do seu projeto, faça checkout dela:

```bash
coast checkout dev-1
```

Isso significa que o runtime agora está em checkout e as portas canônicas do seu projeto (como `3000`, `5432`) serão roteadas para esta instância do Coast.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

Para abrir a UI de observabilidade do Coastguard para o seu projeto:

```bash
coast ui
```

## O que vem a seguir?

- Configure uma [skill para o seu agente host](SKILLS_FOR_HOST_AGENTS.md) para que ele saiba como interagir com o Coasts

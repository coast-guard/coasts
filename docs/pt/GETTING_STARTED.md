# Primeiros Passos com Coasts

```youtube
Je921fgJ4RY
Part of the [Coasts Video Course](learn-coasts-videos/README.md).
```

## Instalando

```bash
curl -fsSL https://coasts.dev/install | sh
coast daemon install
```

*Se você decidir não executar `coast daemon install`, você é responsável por iniciar o daemon manualmente com `coast daemon start` todas as vezes.*

## Requisitos

- macOS ou Linux
- Docker Desktop no macOS, ou Docker Engine com o plugin Compose no Linux
- Um projeto usando Git
- Node.js
- `socat` (`brew install socat` no macOS, `sudo apt install socat` no Ubuntu)

```text
Nota sobre Linux: Portas dinâmicas funcionam imediatamente no Linux.
Se você precisar de portas canônicas abaixo de `1024`, consulte a documentação de checkout para a configuração de host necessária.
```

## Configurando o Coasts em um Projeto

Adicione um Coastfile à raiz do seu projeto. Certifique-se de que você não está em um worktree ao instalar.

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

Um Coastfile é um arquivo TOML leve que *tipicamente* aponta para o seu `docker-compose.yml` existente (ele também funciona com configurações de desenvolvimento local sem contêiner) e descreve as modificações necessárias para executar seu projeto em paralelo — mapeamentos de portas, estratégias de volume e segredos. Coloque-o na raiz do seu projeto.

A maneira mais rápida de criar um Coastfile para o seu projeto é deixar seu agente de codificação fazer isso.

A CLI do Coasts inclui um prompt embutido que ensina a qualquer agente de IA o esquema completo do Coastfile e a CLI. Copie-o no chat do seu agente e ele analisará seu projeto e gerará um Coastfile.

```prompt-copy
installation_prompt.txt
```

Você também pode obter a mesma saída pela CLI executando `coast installation-prompt`.

## Seu Primeiro Coast

Antes de iniciar seu primeiro Coast, derrube qualquer ambiente de desenvolvimento em execução. Se você estiver usando Docker Compose, execute `docker-compose down`. Se você tiver servidores de desenvolvimento locais em execução, pare-os. Coasts gerenciam suas próprias portas e entrarão em conflito com qualquer coisa que já esteja escutando.

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

Cada instância recebe seu próprio conjunto de portas dinâmicas para que múltiplas instâncias possam rodar lado a lado. Para mapear uma instância de volta para as portas canônicas do seu projeto, faça o checkout dela:

```bash
coast checkout dev-1
```

Isso significa que o runtime agora está em checkout e as portas canônicas do seu projeto (como `3000`, `5432`) irão rotear para esta instância do Coast.

```bash
coast ls

# NAME   PROJECT     STATUS   BRANCH  RUNTIME  WORKTREE  CO  ROOT
# dev-1  my-project  running  main    dind     -         ✓   ~/dev/my-project
```

Para abrir a UI de observabilidade do Coastguard para o seu projeto:

```bash
coast ui
```

## O Que Vem a Seguir?

- Configure uma [skill para o seu agente host](SKILLS_FOR_HOST_AGENTS.md) para que ele saiba como interagir com Coasts

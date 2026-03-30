# Caminhos Privados

Quando várias instâncias do Coast compartilham a mesma raiz de projeto, elas compartilham os mesmos arquivos — e os mesmos inodes. Normalmente esse é o objetivo: alterações de arquivos no host aparecem instantaneamente dentro do Coast porque ambos os lados veem o mesmo sistema de arquivos. Mas algumas ferramentas gravam estado por processo no workspace assumindo acesso exclusivo, e essa suposição falha quando duas instâncias compartilham a mesma montagem.

## O Problema

Considere o Next.js 16, que adquire um bloqueio exclusivo em `.next/dev/lock` via `flock(fd, LOCK_EX)` quando o servidor de desenvolvimento inicia. `flock` é um mecanismo do kernel no nível de inode — ele não se importa com namespaces de montagem, limites de contêiner ou caminhos de bind mount. Se dois processos em dois contêineres Coast diferentes apontarem para o mesmo inode de `.next/dev/lock` (porque compartilham o mesmo bind mount do host), o segundo processo verá o bloqueio do primeiro e se recusará a iniciar:

```text
⨯ Another next dev server is already running.

- Local: http://localhost:3000
- PID: 1361
- Dir: /workspace/frontend
```

A mesma categoria de conflito se aplica a:

- bloqueios consultivos `flock` / `fcntl` (Next.js, Turbopack, Cargo, Gradle)
- arquivos PID (muitos daemons gravam um arquivo PID e o verificam na inicialização)
- caches de build que assumem acesso de único gravador (Webpack, Vite, esbuild)

O isolamento de namespace de montagem (`unshare`) não ajuda aqui. Namespaces de montagem controlam quais pontos de montagem um processo pode ver, mas `flock` opera no próprio inode. Dois processos vendo o mesmo inode por caminhos de montagem diferentes ainda entrarão em conflito.

## A Solução

O campo `private_paths` do Coastfile declara diretórios relativos ao workspace que devem ser por instância. Cada instância do Coast recebe seu próprio bind mount isolado para esses caminhos, apoiado por um diretório por instância no próprio sistema de arquivos do contêiner.

```toml
[coast]
name = "my-app"
private_paths = ["frontend/.next"]
```

Depois que o Coast monta `/workspace` com propagação compartilhada, ele aplica um bind mount adicional para cada caminho privado:

```text
mkdir -p /coast-private/frontend/.next /workspace/frontend/.next
mount --bind /coast-private/frontend/.next /workspace/frontend/.next
```

`/coast-private/` vive na camada gravável do contêiner DinD — não no bind mount compartilhado do host — então cada instância naturalmente recebe inodes separados. O arquivo de bloqueio em `dev-1` vive em um inode diferente do arquivo de bloqueio em `dev-2`, e o conflito desaparece.

## Como Funciona

As montagens de caminhos privados são aplicadas em todos os pontos do ciclo de vida do Coast em que `/workspace` é montado ou remontado:

1. **`coast run`** — após o `mount --bind /host-project /workspace && mount --make-rshared /workspace` inicial, os caminhos privados são montados.
2. **`coast start`** — após reaplicar o bind mount do workspace na reinicialização do contêiner.
3. **`coast assign`** — após desmontar e refazer o bind de `/workspace` para um diretório worktree.
4. **`coast unassign`** — após reverter `/workspace` de volta para a raiz do projeto.

Os diretórios privados persistem entre ciclos de parada/inicialização (eles vivem no sistema de arquivos do contêiner, não na montagem compartilhada). Em `coast assign` ou `coast unassign`, os diretórios privados são **limpos** para que os servidores de desenvolvimento recompilem a partir dos arquivos-fonte do branch correto, em vez de servir saída de build obsoleta de um branch anterior. Em `coast rm`, eles são destruídos junto com o contêiner.

## Quando Usar

Use `private_paths` quando uma ferramenta gravar estado por processo ou por instância em um diretório do workspace que entre em conflito entre instâncias Coast concorrentes:

- **Bloqueios de arquivos**: `.next/dev/lock`, `target/.cargo-lock` do Cargo, `.gradle/lock` do Gradle
- **Caches de build**: `.next`, `.turbo`, `target/`, `.vite`
- **Arquivos PID**: qualquer daemon que grave um arquivo PID no workspace

Não use `private_paths` para dados que precisam ser compartilhados entre instâncias ou visíveis no host. Se você precisa de dados persistentes, isolados e gerenciados pelo Docker (como volumes de banco de dados), use [volumes com `strategy = "isolated"`](../coastfiles/VOLUMES.md) em vez disso.

## Regras de Validação

- Os caminhos devem ser relativos (sem `/` inicial)
- Os caminhos não devem conter componentes `..`
- Os caminhos não devem se sobrepor — listar tanto `frontend/.next` quanto `frontend/.next/cache` é um erro porque a primeira montagem ocultaria a segunda

## Relação com Volumes

`private_paths` e `[volumes]` resolvem problemas de isolamento diferentes:

| | `private_paths` | `[volumes]` |
|---|---|---|
| **O quê** | Diretórios relativos ao workspace | Volumes nomeados gerenciados pelo Docker |
| **Onde** | Dentro de `/workspace` | Caminhos de montagem arbitrários no contêiner |
| **Armazenado em** | Sistema de arquivos local do contêiner (`/coast-private/`) | Volumes nomeados do Docker |
| **Isolamento** | Sempre por instância | Estratégia `isolated` ou `shared` |
| **Sobrevive a `coast rm`** | Não | Isolated: não. Shared: sim. |
| **Caso de uso** | Artefatos de build, arquivos de bloqueio, caches | Bancos de dados, dados persistentes da aplicação |

## Referência de Configuração

Consulte [`private_paths`](../coastfiles/PROJECT.md) na referência do Coastfile para a sintaxe completa e exemplos.

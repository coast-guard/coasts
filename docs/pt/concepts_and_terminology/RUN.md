# Executar

`coast run` cria uma nova instância do Coast. Ele resolve a [build](BUILDS.md) mais recente, provisiona um [contêiner DinD](RUNTIMES_AND_SERVICES.md), carrega imagens em cache, inicia seus serviços do compose, aloca [portas dinâmicas](PORTS.md) e registra a instância no banco de dados de estado.

```bash
coast run dev-1
```

Se você passar `-w`, o Coast também [atribui](ASSIGN.md) a worktree após a conclusão do provisionamento:

```bash
coast run dev-1 -w feature/oauth
```

Este é o padrão mais comum quando um harness ou agente cria uma worktree e precisa de um Coast para ela em uma única etapa.

## O que acontece

`coast run` executa quatro fases:

1. **Validar e inserir** — verifica se o nome é único, resolve o ID da build (a partir do symlink `latest` ou de um `--build-id` explícito) e insere um registro de instância `Provisioning`.
2. **Provisionamento do Docker** — cria o contêiner DinD no daemon do host, compila quaisquer imagens por instância, carrega tarballs de imagens em cache no daemon interno, reescreve o arquivo compose, injeta segredos e executa `docker compose up -d`.
3. **Finalizar** — armazena alocações de portas, define a porta primária se houver exatamente uma e faz a transição da instância para `Running`.
4. **Atribuição opcional de worktree** — se `-w <worktree>` foi fornecido, executa `coast assign` na nova instância. Se a atribuição falhar, o Coast ainda estará em execução — a falha é registrada como um aviso.

O volume persistente `/var/lib/docker` dentro do contêiner DinD significa que execuções subsequentes ignoram o carregamento de imagens. Um `coast run` novo com caches frios pode levar mais de 20 segundos; uma nova execução após `coast rm` normalmente termina em menos de 10 segundos.

## Uso da CLI

```text
coast run <name> [options]
```

| Flag | Descrição |
|------|-------------|
| `-w`, `--worktree <name>` | Atribuir esta worktree após a conclusão do provisionamento |
| `--n <count>` | Criação em lote. O nome deve conter `{n}` (por exemplo, `coast run dev-{n} --n=5` cria dev-1 até dev-5) |
| `-t`, `--type <type>` | Usar uma build tipada (por exemplo, `--type snap` resolve `latest-snap` em vez de `latest`) |
| `--force-remove-dangling` | Remover um contêiner Docker remanescente com o mesmo nome antes de criar |
| `-s`, `--silent` | Suprimir a saída de progresso; imprimir apenas o resumo final ou erros |
| `-v`, `--verbose` | Mostrar detalhes verbosos, incluindo logs de build do Docker |

A branch git é sempre detectada automaticamente a partir do HEAD atual.

## Criação em lote

Use `{n}` no nome e `--n` para criar múltiplas instâncias de uma vez:

```bash
coast run dev-{n} --n=5
```

Isso cria `dev-1`, `dev-2`, `dev-3`, `dev-4`, `dev-5` sequencialmente. Cada instância recebe seu próprio contêiner DinD, alocações de portas e estado de volume. Lotes maiores que 10 solicitam confirmação.

## Builds tipadas

Se o seu projeto usa múltiplos tipos de Coastfile (veja [Tipos de Coastfile](COASTFILE_TYPES.md)), passe `--type` para selecionar qual build usar:

```bash
coast run dev-1                    # resolves "latest"
coast run test-1 --type test       # resolves "latest-test"
coast run snapshot-1 --type snap   # resolves "latest-snap"
```

## Executar vs atribuir e remover

- `coast run` cria uma instância **nova**. Use-o quando você precisar de outro Coast.
- `coast assign` redireciona uma instância **existente** para uma worktree diferente. Use-o
  quando você já tiver um Coast e quiser trocar qual código ele executa.
- `coast rm` desmonta uma instância completamente. Use-o quando quiser encerrar
  Coasts ou recriar um do zero.

Na maioria das trocas do dia a dia, não é necessário usar `coast rm`; `coast assign` e
`coast checkout` geralmente são suficientes. Recorra a `coast rm` quando quiser uma
recriação limpa, especialmente após recompilar seu Coastfile ou build.

Você pode combiná-los: `coast run dev-3 -w feature/billing` cria a instância
e atribui a worktree em uma única etapa.

## Contêineres remanescentes

Se um `coast run` anterior foi interrompido ou `coast rm` não limpou tudo completamente, você pode ver um erro de "contêiner Docker remanescente". Passe `--force-remove-dangling` para remover o contêiner remanescente e prosseguir:

```bash
coast run dev-1 --force-remove-dangling
```

## Veja também

- [Remove](REMOVE.md) — desmontando uma instância completamente
- [Builds](BUILDS.md) — o que `coast run` consome
- [Runtimes and Services](RUNTIMES_AND_SERVICES.md) — a arquitetura DinD dentro de cada instância
- [Assign and Unassign](ASSIGN.md) — alternando uma instância existente para uma worktree diferente
- [Ports](PORTS.md) — como portas dinâmicas e canônicas são alocadas
- [Coasts](COASTS.md) — o conceito de alto nível de uma instância Coast

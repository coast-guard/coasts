# Remover

`coast rm` desmonta completamente uma instância do Coast. Ele para a instância se ela
estiver em execução, remove o contêiner DinD, exclui volumes isolados, desaloca
portas, remove shells do agente e exclui a instância do estado.

```bash
coast rm dev-1
```

A maioria dos fluxos de trabalho do dia a dia não precisa de `coast rm`. Se você só quer que um Coast
execute código diferente ou detenha as portas canônicas, use [Atribuir e
Desatribuir](ASSIGN.md) ou [Checkout](CHECKOUT.md) em vez disso. Recorra a `coast rm`
quando quiser desativar Coasts, recuperar estado de execução por instância ou
recriar uma instância do zero após reconstruir seu Coastfile ou build.

## O que acontece

`coast rm` executa cinco fases:

1. **Validar e localizar** — procura a instância no estado. Se o registro de
   estado tiver desaparecido, mas ainda existir um contêiner pendente com o nome esperado,
   `coast rm` limpa isso também.
2. **Parar se necessário** — se a instância estiver em `Running` ou `CheckedOut`, o Coast
   derruba primeiro a stack compose interna e para o contêiner DinD.
3. **Remover artefatos de execução** — remove o contêiner Coast e exclui
   volumes isolados dessa instância.
4. **Limpar estado do host** — encerra encaminhadores de porta remanescentes, desaloca
   portas, remove shells do agente e exclui o registro da instância do banco de dados
   de estado.
5. **Preservar dados compartilhados** — volumes de serviço compartilhados e dados de serviço compartilhados
   são deixados intactos.

## Uso da CLI

```text
coast rm <name>
coast rm --all
```

| Flag | Descrição |
|------|-------------|
| `<name>` | Remove uma instância pelo nome |
| `--all` | Remove todas as instâncias do projeto atual |

`coast rm --all` resolve o projeto atual, lista suas instâncias e as remove
uma por uma. Se não houver instâncias, ele sai normalmente.

## Serviços compartilhados e builds

- `coast rm` **não** exclui dados de serviços compartilhados.
- Use `coast shared-services rm <service>` se também quiser remover um serviço
  compartilhado e seus dados.
- Use `coast rm-build` se quiser remover artefatos de build após desmontar
  instâncias.

## Quando usar

- após reconstruir seu Coastfile ou criar um novo build e querer uma instância
  nova
- quando quiser desativar Coasts e liberar estado de contêiner e volume por instância
- quando uma instância estiver travada e começar do zero for mais fácil do que depurá-la
  no local

## Veja também

- [Run](RUN.md) — criando uma nova instância do Coast
- [Assign and Unassign](ASSIGN.md) — redirecionando uma instância existente para uma
  worktree diferente
- [Shared Services](SHARED_SERVICES.md) — o que `coast rm` não exclui
- [Builds](BUILDS.md) — artefatos de build e `coast rm-build`

# Solução de problemas

A maioria dos problemas com o Coasts vem de estado desatualizado, recursos do Docker órfãos ou um daemon que saiu de sincronia. Esta página cobre o caminho de escalonamento do mais leve ao nuclear.

## Doctor

Se as coisas parecerem estranhas — instâncias aparecem como em execução, mas nada responde; portas parecem travadas; ou a UI mostra dados desatualizados — comece com `coast doctor`:

```bash
coast doctor
```

O Doctor verifica o banco de dados de estado e o Docker em busca de inconsistências: registros de instâncias órfãos com contêineres ausentes, contêineres pendentes sem registro de estado e serviços compartilhados marcados como em execução que na verdade estão mortos. Ele corrige automaticamente o que encontrar.

Para visualizar o que ele faria sem alterar nada:

```bash
coast doctor --dry-run
```

## Reinício do daemon

Se o próprio daemon parecer não responsivo ou você suspeitar que ele está em um estado ruim, reinicie-o:

```bash
coast daemon restart
```

Isso envia um sinal de desligamento gracioso, espera o daemon encerrar e inicia um processo novo. Suas instâncias e seu estado são preservados.

## Removendo um único projeto

Se o problema estiver isolado a um projeto, você pode remover seus artefatos de build e os recursos do Docker associados sem afetar mais nada:

```bash
coast rm-build my-project
```

Isso exclui o diretório de artefatos do projeto, imagens do Docker, volumes e contêineres. Ele pede confirmação primeiro. Passe `--force` para pular o prompt.

## Restauração de fábrica com Nuke

Quando nada mais funciona — ou você só quer uma ficha completamente limpa — `coast nuke` executa uma restauração completa de fábrica:

```bash
coast nuke
```

Isso irá:

1. Parar o daemon `coastd`.
2. Remover **todos** os contêineres do Docker gerenciados pelo coast.
3. Remover **todos** os volumes do Docker gerenciados pelo coast.
4. Remover **todas** as redes do Docker gerenciadas pelo coast.
5. Remover **todas** as imagens do Docker do coast.
6. Excluir todo o diretório `~/.coast/` (banco de dados de estado, builds, logs, segredos, cache de imagens).
7. Recriar `~/.coast/` e reiniciar o daemon para que o coast fique imediatamente utilizável novamente.

Como isso destrói tudo, você deve digitar `nuke` no prompt de confirmação:

```text
$ coast nuke
WARNING: This will permanently destroy ALL coast data:

  - Stop the coastd daemon
  - Remove all coast-managed Docker containers
  - Remove all coast-managed Docker volumes
  - Remove all coast-managed Docker networks
  - Remove all coast Docker images
  - Delete ~/.coast/ (state DB, builds, logs, secrets, image cache)

Type "nuke" to confirm:
```

Passe `--force` para pular o prompt (útil em scripts):

```bash
coast nuke --force
```

Depois de um nuke, o coast está pronto para uso — o daemon está em execução e o diretório home existe. Você só precisa executar `coast build` e `coast run` nos seus projetos novamente.

## Relatando bugs

Se você encontrar um problema que não seja resolvido por nenhum dos itens acima, inclua os logs do daemon ao relatar:

```bash
coast daemon logs
```

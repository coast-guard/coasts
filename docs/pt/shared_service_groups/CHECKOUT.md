# Checkout do SSG no Lado do Host

Os Consumer Coasts alcançam os serviços SSG por meio da camada de roteamento do daemon (socat in-DinD -> socat do host -> porta dinâmica). Isso funciona muito bem para contêineres de app. Isso não ajuda chamadores no lado do host -- MCPs, sessões ad-hoc de `psql`, o inspetor de banco de dados do seu editor -- que querem se conectar a `localhost:5432` como se o serviço estivesse bem ali.

`coast ssg checkout` resolve isso. Ele inicia um socat no nível do host que faz bind da porta canônica do host (5432 para Postgres, 6379 para Redis, ...) e encaminha para a porta virtual estável do projeto. A partir daí, o socat de porta virtual já existente no host carrega o tráfego até a porta dinâmica atualmente publicada do SSG.

Tudo isso é por projeto. `coast ssg checkout --service postgres` resolve para o projeto que possui o `Coastfile` do cwd; se você tiver dois projetos nesta máquina, apenas um poderá manter a porta canônica 5432 por vez.

## Uso

```bash
coast ssg checkout --service postgres     # bind de um serviço
coast ssg checkout --all                  # bind de todos os serviços SSG
coast ssg uncheckout --service postgres   # desmonta um
coast ssg uncheckout --all                # desmonta todos os checkouts ativos
```

Após um checkout bem-sucedido, `coast ssg ports` anota cada serviço associado com `(checked out)`:

```text
  SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
  postgres             5432            54201           42000      (checked out)
  redis                6379            54202           42001
```

Os Consumer Coasts sempre alcançam os serviços SSG por meio da sua cadeia socat in-DinD -> porta virtual, independentemente do estado do checkout no lado do host. Checkout é puramente uma conveniência no lado do host.

## Encaminhador de Dois Saltos

O socat de checkout **não** aponta diretamente para a porta dinâmica do host do SSG. Ele aponta para a porta virtual estável do projeto:

```text
processo no host       -> 127.0.0.1:5432           (socat de checkout, escuta aqui)
                        -> 127.0.0.1:42000          (porta virtual do projeto)
                        -> 127.0.0.1:54201          (porta dinâmica atual do SSG)
                        -> <project>-ssg postgres   (serviço interno)
```

A cadeia de dois saltos significa que o socat de checkout continua funcionando ao longo de rebuilds do SSG, mesmo que a porta dinâmica mude. Apenas o socat da porta virtual do host é atualizado -- o socat da porta canônica não sabe disso. Veja [Routing](ROUTING.md) para entender como a camada socat do host é mantida.

## Deslocamento de Detentores de Instâncias Coast

Quando você pede ao SSG para fazer checkout de uma porta canônica, essa porta talvez já esteja ocupada. A semântica depende de quem a ocupa:

- **Uma instância Coast que foi explicitamente colocada em checkout.** `coast checkout <instance>` em algum Coast mais cedo hoje fez bind de `localhost:5432` ao Postgres interno daquele Coast. O checkout do SSG **a desloca**: o daemon mata o socat existente, limpa `port_allocations.socat_pid` para aquele Coast e faz bind do socat do SSG no lugar. A CLI imprime um aviso claro:

  ```text
  Warning: displaced coast 'my-app/dev-2' from canonical port 5432.
  SSG checkout complete: postgres on canonical 5432 -> virtual 42000.
  ```

  O Coast deslocado **não** é religado automaticamente quando você mais tarde executa `coast ssg uncheckout`. Sua porta dinâmica continua funcionando, mas a porta canônica permanece sem bind até que você execute `coast checkout my-app/dev-2` novamente.

- **O checkout de SSG de outro projeto.** Se `filemap-ssg` já tiver feito checkout da 5432 e você tentar fazer checkout da 5432 de `cg-ssg`, o daemon recusa com uma mensagem clara nomeando quem a detém. Faça uncheckout da 5432 de `filemap-ssg` primeiro.

- **Uma linha de checkout de SSG anterior com `socat_pid` morto.** Metadados obsoletos de um daemon que travou ou de um ciclo de parar/iniciar. O novo checkout recupera silenciosamente a linha.

- **Qualquer outra coisa** (um Postgres no host que você iniciou manualmente, outro daemon, `nginx` na porta 8080). `coast ssg checkout` falha:

  ```text
  error: port 5432 is held by a process outside Coast's tracking. Free the port with `lsof -i :5432` / `kill <pid>` and retry.
  ```

  Não há flag `--force`. Matar silenciosamente um processo desconhecido foi considerado perigoso demais.

## Comportamento de Stop / Start

`coast ssg stop` mata os processos socat ativos da porta canônica, mas **preserva as próprias linhas de checkout** no banco de estado.

`coast ssg run` / `start` / `restart` iteram pelas linhas preservadas e recriam um socat novo de porta canônica por linha. A porta canônica (5432) permanece idêntica; apenas a porta dinâmica muda entre ciclos de `run`, e como o socat de checkout aponta para a porta **virtual** (que também é estável), o rebind é mecânico.

Se um serviço desaparecer do SSG reconstruído, sua linha de checkout é removida com um aviso na resposta do run:

```text
SSG run complete. Dropped stale checkout for service 'mongo' (no longer in the active SSG build).
```

`coast ssg rm` apaga todas as linhas `ssg_port_checkouts` do projeto. `rm` é destrutivo por design -- você explicitamente pediu um estado limpo.

## Recuperação Após Reinício do Daemon

Após um reinício inesperado do daemon (falha, `coastd restart`, reboot), `restore_running_state` consulta a tabela `ssg_port_checkouts` e recria cada linha com base na alocação atual de portas dinâmica / virtual. Seu `localhost:5432` permanece associado ao longo de reinicializações do daemon.

## Quando Fazer Checkout

- Você quer apontar um cliente GUI de banco de dados para o Postgres SSG do projeto.
- Você quer que `psql "postgres://coast:coast@localhost:5432/mydb"` funcione sem precisar descobrir a porta dinâmica primeiro.
- Um MCP no seu host precisa de um endpoint canônico estável.
- O Coastguard quer fazer proxy da porta de admin HTTP do SSG.

Quando **não** fazer checkout:

- Para conectividade de dentro de um Consumer Coast -- isso já funciona via socat in-DinD para porta virtual.
- Quando você está satisfeito em usar a saída de `coast ssg ports` e inserir a porta dinâmica na sua ferramenta.

## Veja Também

- [Routing](ROUTING.md) -- os conceitos de porta canônica / dinâmica / virtual e a cadeia completa de encaminhamento no lado do host
- [Lifecycle](LIFECYCLE.md) -- detalhes de stop / start / rm
- [Coast Checkout](../concepts_and_terminology/CHECKOUT.md) -- a versão para instância Coast desta ideia
- [Ports](../concepts_and_terminology/PORTS.md) -- o encanamento de portas canônicas vs dinâmicas em todo o sistema

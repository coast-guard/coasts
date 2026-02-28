# Daemon do Coast

O daemon do Coast (`coastd`) é o processo local de longa duração que faz o trabalho real de orquestração. O [CLI](CLI.md) e o [Coastguard](COASTGUARD.md) são clientes; `coastd` é o plano de controle por trás deles.

## Arquitetura em um Relance

```text
coast CLI (automation) -----+
                            +--> coastd daemon
Coastguard UI (human) ------+         |
                                      +--> Coasts
                                      +--> Ports
                                      +--> State
```

O CLI envia solicitações ao `coastd` por meio de um socket Unix local; o Coastguard se conecta por um WebSocket. O daemon aplica alterações ao estado em tempo de execução.

## O Que Ele Faz

O `coastd` lida com as operações que precisam de estado persistente e coordenação em segundo plano:

- Rastreia instâncias do Coast, builds e serviços compartilhados.
- Cria, inicia, para e remove runtimes do Coast.
- Aplica operações de assign/unassign/checkout.
- Gerencia o [encaminhamento de portas](PORTS.md) canônico e dinâmico.
- Transmite [logs](LOGS.md), status e eventos de runtime para clientes CLI e UI.

Em resumo: se você executar `coast run`, `coast assign`, `coast checkout` ou `coast ls`, o daemon é o componente que está fazendo o trabalho.

## Como Ele Roda

Você pode executar o daemon de duas formas comuns:

```bash
# Register daemon auto-start at login (recommended)
coast daemon install

# Manual start mode
coast daemon start
```

Se você pular o daemon install, precisa iniciá-lo manualmente a cada sessão antes de usar os comandos do Coast.

## Relatando Bugs

Se você encontrar problemas, inclua os logs do daemon `coastd` ao enviar um relatório de bug. Os logs contêm o contexto necessário para diagnosticar a maioria dos problemas:

```bash
coast daemon logs
```

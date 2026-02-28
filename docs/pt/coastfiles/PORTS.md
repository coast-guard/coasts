# Portas

A seção `[ports]` declara quais portas o Coast gerencia para encaminhamento entre suas instâncias do Coast e a máquina host. A seção opcional `[egress]` declara portas no host que as instâncias do Coast precisam alcançar para tráfego de saída.

Para entender como o encaminhamento de portas funciona em tempo de execução — portas canônicas vs dinâmicas, troca de checkout, socat — veja [Ports](../concepts_and_terminology/PORTS.md) e [Checkout](../concepts_and_terminology/CHECKOUT.md).

## `[ports]`

Um mapa plano de `logical_name = port_number`. Cada entrada diz ao Coast para configurar o encaminhamento de portas para essa porta quando uma instância do Coast é executada.

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

Cada instância recebe uma porta dinâmica (faixa alta, sempre acessível) para cada porta declarada. A instância em [checked-out](../concepts_and_terminology/CHECKOUT.md) também recebe a porta canônica (o número que você declarou) encaminhada para o host.

Regras:

- Os valores de porta devem ser inteiros sem sinal de 16 bits, não zero (1-65535).
- Nomes lógicos são strings livres usadas como identificadores em `coast ports`, Coastguard e `primary_port`.

### Exemplo mínimo

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 3000
```

### Exemplo com múltiplos serviços

```toml
[ports]
web = 3000
api = 4000
backend = 8080
postgres = 5432
redis = 6379
```

## `primary_port`

Definido na seção `[coast]` (documentada em [Project and Setup](PROJECT.md)), `primary_port` nomeia uma das suas portas declaradas para links rápidos e roteamento por subdomínio no [Coastguard](../concepts_and_terminology/COASTGUARD.md).

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

O valor deve corresponder a uma chave em `[ports]`. Veja [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) para detalhes.

## `[egress]`

Declara portas no host que as instâncias do Coast precisam alcançar. Esta é a direção inversa de `[ports]` — em vez de encaminhar uma porta *para fora* do Coast para o host, o egress torna uma porta do host alcançável *de dentro* do Coast.

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 48090

[egress]
host-api = 48080
```

Isso é útil quando seus serviços do compose dentro de um Coast precisam falar com algo executando diretamente na máquina host (fora do sistema de serviços compartilhados do Coast).

Regras:

- Igual a `[ports]`: os valores devem ser inteiros sem sinal de 16 bits, não zero.
- Nomes lógicos são identificadores livres.

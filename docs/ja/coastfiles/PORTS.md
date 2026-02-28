# ポート

`[ports]` セクションは、Coast インスタンスとホストマシン間の転送を Coast が管理するポートを宣言します。オプションの `[egress]` セクションは、Coast インスタンスが外向きに到達する必要があるホスト上のポートを宣言します。

実行時にポートフォワーディングがどのように動作するか（カノニカル vs ダイナミックポート、チェックアウトの切り替え、socat）については、[Ports](../concepts_and_terminology/PORTS.md) および [Checkout](../concepts_and_terminology/CHECKOUT.md) を参照してください。

## `[ports]`

`logical_name = port_number` のフラットなマップ。各エントリは、Coast インスタンスが実行されるときに、そのポートに対するポートフォワーディングを設定するよう Coast に指示します。

```toml
[ports]
web = 3000
api = 8080
postgres = 5432
```

各インスタンスは、宣言された各ポートごとに（高い範囲で、常にアクセス可能な）ダイナミックポートを取得します。[checked-out](../concepts_and_terminology/CHECKOUT.md) インスタンスは、ホストへ転送されるカノニカルポート（宣言した番号）も取得します。

ルール:

- ポート値は 0 ではない符号なし 16 ビット整数（1-65535）でなければなりません。
- 論理名は、`coast ports`、Coastguard、`primary_port` で識別子として使用される自由形式の文字列です。

### 最小例

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 3000
```

### マルチサービス例

```toml
[ports]
web = 3000
api = 4000
backend = 8080
postgres = 5432
redis = 6379
```

## `primary_port`

`[coast]` セクション（[Project and Setup](PROJECT.md) に記載）で設定する `primary_port` は、[Coastguard](../concepts_and_terminology/COASTGUARD.md) におけるクイックリンクおよびサブドメインルーティングのために、宣言したポートのうち 1 つに名前を付けます。

```toml
[coast]
name = "my-app"
primary_port = "web"

[ports]
web = 3000
api = 8080
```

値は `[ports]` 内のキーと一致しなければなりません。詳細は [Primary Port and DNS](../concepts_and_terminology/PRIMARY_PORT_AND_DNS.md) を参照してください。

## `[egress]`

Coast インスタンスが到達する必要があるホスト上のポートを宣言します。これは `[ports]` とは逆方向です。つまり、Coast からホストへポートを *外向き* に転送するのではなく、egress はホストのポートを Coast の *内部から* 到達可能にします。

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
app = 48090

[egress]
host-api = 48080
```

これは、Coast 内の compose サービスが、ホストマシン上で直接動作しているもの（Coast の共有サービスシステムの外側）と通信する必要がある場合に有用です。

ルール:

- `[ports]` と同様: 値は 0 ではない符号なし 16 ビット整数でなければなりません。
- 論理名は自由形式の識別子です。

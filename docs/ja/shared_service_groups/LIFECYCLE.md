# SSG ライフサイクル

各プロジェクトの SSG は、それぞれ独自の外側の Docker-in-Docker コンテナであり、名前は `<project>-ssg` です（例: `cg-ssg`）。ライフサイクルの各動詞は、cwd の `Coastfile` を所有するプロジェクト（または `--working-dir` で指定されたプロジェクト）の SSG を対象にします。すべての変更系コマンドは、デーモン上のプロジェクト単位の mutex を介して直列化されるため、同じプロジェクトに対する 2 つの同時 `coast ssg run` / `coast ssg stop` 呼び出しは競合せずにキューイングされます -- ただし、異なる 2 つのプロジェクトはそれぞれの SSG を並列に変更できます。

## 状態マシン

```text
                     coast ssg build           coast ssg run
(no build)   -->  built     -->     created    -->     running
                                                          |
                                                   coast ssg stop
                                                          v
                                                       stopped
                                                          |
                                                  coast ssg start
                                                          v
                                                       running
                                                          |
                                                   coast ssg rm
                                                          v
                                                      (removed)
```

- `coast ssg build` はコンテナを作成しません。これは `~/.coast/ssg/<project>/builds/<id>/` 配下のディスク上にアーティファクトを生成し、`[secrets.*]` が宣言されている場合は秘密値をキーストアに抽出します。
- `coast ssg run` は `<project>-ssg` DinD を作成し、動的ホストポートを割り当て、宣言された秘密を実行ごとの `compose.override.yml` に具体化し、内側の compose スタックを起動します。
- `coast ssg stop` は外側の DinD を停止しますが、コンテナ、動的ポート行、およびプロジェクト単位の仮想ポートは保持されるため、`start` は高速です。
- `coast ssg start` は SSG を再起動し、秘密を再度具体化します（そのため、stop と start の間に `coast ssg secrets clear` を実行すると反映されます）。
- `coast ssg rm` は外側の DinD コンテナを削除します。`--with-data` を付けると、内側の名前付きボリュームも削除されます（ホストの bind-mount 内容には一切触れません）。キーストアが `rm` によってクリアされることはありません -- それを行うのは `coast ssg secrets clear` だけです。
- `coast ssg restart` は `stop` + `start` の便利ラッパーです。

## コマンド

### `coast ssg run`

`<project>-ssg` DinD が存在しない場合は作成し、その内側のサービスを起動します。宣言された各サービスにつき 1 つの動的ホストポートを割り当て、それらを外側の DinD 上で公開します。ポートアロケータが再利用しないように、そのマッピングを state DB に書き込みます。

```bash
coast ssg run
```

`coast ssg build` と同じ `BuildProgressEvent` チャネルを通じて進行イベントをストリームします。デフォルトのプランは 7 ステップです:

1. SSG を準備中
2. SSG コンテナを作成中
3. SSG コンテナを起動中
4. 内側のデーモンを待機中
5. キャッシュ済みイメージを読み込み中
6. 秘密を具体化中（`[secrets]` ブロックがない場合は無言、ある場合は秘密ごとの項目を出力）
7. 内側のサービスを起動中

**自動起動**。SSG サービスを参照する consumer Coast 上で `coast run` を実行すると、SSG がまだ動作していなければ自動的に起動します。いつでも明示的に `coast ssg run` を実行できますが、必要になることはほとんどありません。[Consuming -> Auto-start](CONSUMING.md#auto-start) を参照してください。

### `coast ssg start`

以前に停止した SSG を起動します。既存の `<project>-ssg` コンテナ（つまり事前の `coast ssg run`）が必要です。stop 以降の変更を反映するためにキーストアから秘密を再具体化し、その後、stop 前に checkout されていた canonical port に対してホスト側の checkout socat を再生成します。

```bash
coast ssg start
```

### `coast ssg stop`

外側の DinD コンテナを停止します。それに伴って内側の compose スタックも停止します。コンテナ、動的ポート割り当て、およびプロジェクト単位の仮想ポート行は保持されるため、次の `start` は高速です。

```bash
coast ssg stop
coast ssg stop --force
```

ホスト側の checkout socat は停止されますが、state DB 内のその行は保持されます。次の `coast ssg start` または `coast ssg run` でそれらが再生成されます。[Checkout](CHECKOUT.md) を参照してください。

**リモート consumer ゲート。** リモート shadow Coast（`coast assign --remote ...` で作成されたもの）が現在それを消費している間、デーモンは SSG の停止を拒否します。`--force` を渡すと、逆方向 SSH トンネルを破棄してそのまま続行します。[Consuming -> Remote Coasts](CONSUMING.md#remote-coasts) を参照してください。

### `coast ssg restart`

`stop` + `start` と同等です。コンテナと動的ポートマッピングは保持されます。

```bash
coast ssg restart
```

### `coast ssg rm`

外側の DinD コンテナを削除します。デフォルトでは内側の名前付きボリューム（Postgres WAL など）は保持されるため、`rm` / `run` のサイクルをまたいでもデータは維持されます。ホストの bind-mount 内容には一切触れません。

```bash
coast ssg rm                    # 名前付きボリュームを保持; キーストアも保持
coast ssg rm --with-data        # 名前付きボリュームも削除; それでもキーストアは保持
coast ssg rm --force            # リモート consumer がいても続行
```

- `--with-data` は、DinD 自体を削除する前に、内側のすべての名前付きボリュームを削除します。新しいデータベースが欲しい場合に使用してください。
- `--force` は、リモート shadow Coast が SSG を参照している場合でも続行します。意味論は `stop --force` と同じです。
- `rm` は `ssg_port_checkouts` 行をクリアします（canonical-port のホストバインディングに対して破壊的です）。

SSG ネイティブの秘密が保存されるキーストア -- (`coast_image = "ssg:<project>"`) -- は、`rm` や `rm --with-data` の影響を**受けません**。SSG の秘密を消去するには、`coast ssg secrets clear` を使用してください（[Secrets](SECRETS.md) を参照）。

### `coast ssg ps`

現在のプロジェクトの SSG のサービス状態を表示します。ビルド済み構成については `manifest.json` を読み取り、実行中コンテナのメタデータについては live state DB を調べます。

```bash
coast ssg ps
```

`run` 成功後の出力:

```text
SSG build: b455787d95cfdeb_20260420061903  (project: cg, running)

  SERVICE              IMAGE                          PORT       STATUS
  postgres             postgres:16                    5432       running
  redis                redis:7-alpine                 6379       running
```

### `coast ssg ports`

サービスごとの canonical / dynamic / virtual ポートマッピングを表示し、そのサービスに対してホスト側の canonical-port socat が有効な場合は `(checked out)` 注記を付けます。virtual port は、consumer が実際に接続するポートです。詳細は [Routing](ROUTING.md) を参照してください。

```bash
coast ssg ports

#   SERVICE              CANONICAL       DYNAMIC         VIRTUAL    STATUS
#   postgres             5432            54201           42000      (checked out)
#   redis                6379            54202           42001
```

### `coast ssg logs`

外側の DinD コンテナ、または特定の内側サービスのログをストリームします。

```bash
coast ssg logs --tail 100
coast ssg logs --service postgres --tail 50
coast ssg logs --service postgres --follow
```

- `--service <name>` は compose キーで内側サービスを対象にします。これがない場合は外側の DinD の stdout を取得します。
- `--tail N` は過去の行数を制限します（デフォルト 200）。
- `--follow` / `-f` は、新しい行が到着するたびに `Ctrl+C` までストリームします。

### `coast ssg exec`

外側の DinD または内側サービスの中でコマンドを実行します。

```bash
coast ssg exec -- sh
coast ssg exec --service postgres -- psql -U coast -l
```

- `--service` なしでは、コマンドは外側の `<project>-ssg` コンテナで実行されます。
- `--service <name>` を付けると、そのコマンドは `docker compose exec -T` を介してその compose サービス内で実行されます。
- `--` より後ろのすべては、フラグを含めて基盤となる `docker exec` にそのまま渡されます。

### `coast ssg ls`

デーモンが把握しているすべての SSG を、すべてのプロジェクトにわたって一覧表示します。これは cwd からプロジェクトを解決しない唯一の動詞であり、デーモンの SSG state 内のすべてのエントリについて行を返します。

```bash
coast ssg ls

#   PROJECT     STATUS     BUILD                                       SERVICES   CREATED
#   cg          running    b455787d95cfdeb_20260420061903               2          2026-04-20T06:19:03Z
#   filemap     stopped    b9b93fdb41b21337_20260418123012               3          2026-04-18T12:30:12Z
```

古いプロジェクトから放置された SSG を見つけたり、このマシン上のどのプロジェクトが何らかの状態の SSG を持っているかを素早く確認したりするのに便利です。

## Mutex の意味論

すべての変更系 SSG 動詞（`run`/`start`/`stop`/`restart`/`rm`/`checkout`/`uncheckout`）は、実際のハンドラへディスパッチする前に、デーモン内部でプロジェクト単位の SSG mutex を取得します。同じプロジェクトに対する 2 つの同時呼び出しはキューイングされ、異なるプロジェクトに対しては並列に実行されます。読み取り専用の動詞（`ps`/`ports`/`logs`/`exec`/`doctor`/`ls`）は mutex を取得しません。

## Coastguard 統合

[Coastguard](../concepts_and_terminology/COASTGUARD.md) を実行している場合、SPA は SSG ライフサイクルを専用ページ（`/project/<p>/ssg/local`）に表示し、Exec、Ports、Services、Logs、Secrets、Stats、Images、Volumes のタブを備えます。`CoastEvent::SsgStarting` と `CoastEvent::SsgStarted` は、consumer Coast が自動起動をトリガーするたびに発火するため、UI はその起動を必要としたプロジェクトに帰属させることができます。

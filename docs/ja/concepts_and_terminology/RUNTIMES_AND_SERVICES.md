# ランタイムとサービス

Coast はコンテナランタイム内 — 自身の Docker（または Podman）デーモンをホストする外側のコンテナ — で実行されます。プロジェクトのサービスはその内側のデーモン内で動作し、他の Coast インスタンスから完全に隔離されます。現在、**本番環境でテスト済みのランタイムは DinD（Docker-in-Docker）のみです。** 現時点では、Podman と Sysbox のサポートが十分にテストされるまで DinD を使用し続けることを推奨します。

## ランタイム

Coastfile の `runtime` フィールドで、Coast を支えるコンテナランタイムを選択します。デフォルトは `dind` で、完全に省略することもできます。

```toml
[coast]
name = "my-app"
runtime = "dind"
```

指定可能な値は `dind`、`sysbox`、`podman` の 3 つです。実際には、DinD のみがデーモンに接続されており、エンドツーエンドでテストされています。

### DinD（Docker-in-Docker）

現時点で使用すべきデフォルトかつ唯一のランタイムです。Coast は `docker:dind` イメージから `--privileged` モードを有効にしてコンテナを作成します。そのコンテナ内で完全な Docker デーモンが起動し、`docker-compose.yml` のサービスがネストされたコンテナとして実行されます。

DinD は完全に統合されています。

- ホスト側でイメージが事前キャッシュされ、`coast run` 時に内側のデーモンへロードされます
- インスタンスごとのイメージはホストでビルドされ、`docker save | docker load` でパイプして取り込まれます
- 内側デーモンの状態は `/var/lib/docker` の名前付きボリューム（`coast-dind--{project}--{instance}`）に永続化されるため、以降の実行ではイメージロード自体が完全にスキップされます
- ポートは DinD コンテナからホストへ直接公開されます
- Compose のオーバーライド、共有サービスのネットワークブリッジ、シークレット注入、ボリューム戦略がすべて動作します

### Sysbox（将来）

Sysbox は Linux 専用の OCI ランタイムで、`--privileged` なしの rootless コンテナを提供します。特権モードの代わりに `--runtime=sysbox-runc` を使用するため、より良いセキュリティ姿勢になります。トレイト実装はコードベース内に存在しますが、デーモンには接続されていません。macOS では動作しません。

### Podman（将来）

Podman は、内側の Docker デーモンを `quay.io/podman/stable` 内で動作する Podman デーモンに置き換え、`docker compose` の代わりに `podman-compose` を使用します。トレイト実装は存在しますが、デーモンには接続されていません。

Sysbox と Podman のサポートが安定したら、このページは更新されます。現時点では `runtime` は `dind` のままにするか、省略してください。

## Docker-in-Docker アーキテクチャ

各 Coast はネストされたコンテナです。ホストの Docker デーモンが外側の DinD コンテナを管理し、その内側にある Docker デーモンが compose サービスを管理します。

```text
Host machine
│
├── Docker daemon (host)
│   │
│   ├── coast container: dev-1 (docker:dind, --privileged)
│   │   │
│   │   ├── Inner Docker daemon
│   │   │   ├── web        (your app, :3000)
│   │   │   ├── postgres   (database, :5432)
│   │   │   └── redis      (cache, :6379)
│   │   │
│   │   ├── /workspace          ← bind mount of your project root
│   │   ├── /image-cache        ← read-only mount of ~/.coast/image-cache/
│   │   ├── /coast-artifact     ← read-only mount of the build artifact
│   │   ├── /coast-override     ← generated compose overrides
│   │   └── /var/lib/docker     ← named volume (inner daemon state)
│   │
│   ├── coast container: dev-2 (docker:dind, --privileged)
│   │   └── (same structure, fully isolated)
│   │
│   └── shared postgres (host-level, bridge network)
│
└── ~/.coast/
    ├── image-cache/    ← OCI tarballs shared across all projects
    └── state.db        ← instance metadata
```

`coast run` がインスタンスを作成するとき、以下を行います。

1. ホストデーモン上で DinD コンテナを作成して起動する
2. 内側のデーモンの準備ができるまで、コンテナ内で `docker info` をポーリングする（最大 120 秒）
3. 内側のデーモンがすでに持っているイメージ（永続化された `/var/lib/docker` ボリューム由来）を確認し、足りない tarball をキャッシュからロードする
4. ホストでビルドしたインスタンスごとのイメージを `docker save | docker load` でパイプして取り込む
5. `/host-project` を `/workspace` にバインドし、compose サービスからソースコードが見えるようにする
6. コンテナ内で `docker compose up -d` を実行し、すべてのサービスが起動中または healthy になるまで待機する

永続化された `/var/lib/docker` ボリュームが最重要の最適化です。新規の `coast run` では、内側のデーモンにイメージをロードするのに 20 秒以上かかることがあります。以降の実行では（`coast rm` 後に再実行しても）、内側のデーモンにイメージがすでにキャッシュされているため、起動は 10 秒未満に短縮されます。

## サービス

サービスとは、Coast 内で動作するコンテナ（または [bare services](BARE_SERVICES.md) の場合はプロセス）です。compose ベースの Coast では、`docker-compose.yml` に定義されたサービスが該当します。

![Services tab in Coastguard](../../assets/coastguard-services.png)
*Coastguard の Services タブ。compose サービス、そのステータス、イメージ、ポートマッピングを表示します。*

Coastguard の Services タブには、Coast インスタンス内で実行中のすべてのサービスが表示されます。

- **Service** — compose のサービス名（例: `web`、`backend`、`redis`）。クリックすると、そのコンテナの詳細な inspect データ、ログ、統計を確認できます。
- **Status** — サービスが実行中か、停止中か、エラー状態か。
- **Image** — サービスが元にしている Docker イメージ。
- **Ports** — 生の compose ポートマッピングと、coast 管理の [canonical/dynamic ports](PORTS.md)。動的ポートは常にアクセス可能で、canonical ポートは [チェックアウト](CHECKOUT.md) されているインスタンスにのみルーティングされます。

複数のサービスを選択し、ツールバーから一括で停止、開始、再起動、削除できます。

[shared services](SHARED_SERVICES.md) として設定されたサービスは、Coast 内ではなくホストデーモン上で実行されるため、この一覧には表示されません。専用のタブがあります。

## `coast ps`

Services タブに相当する CLI は `coast ps` です。

```bash
coast ps dev-1
```

```text
Services in coast instance 'dev-1':
  NAME                      STATUS               PORTS
  backend                   running              0.0.0.0:8080->8080/tcp, 0.0.0.0:40000->40000/tcp
  mailhog                   running              0.0.0.0:1025->1025/tcp, 0.0.0.0:8025->8025/tcp
  reach-web                 running              0.0.0.0:4000->4000/tcp
  test-redis                running              0.0.0.0:6380->6379/tcp
  web                       running              0.0.0.0:3000->3000/tcp
```

内部的には、デーモンが DinD コンテナ内で `docker compose ps --format json` を実行し、JSON 出力を解析します。結果は返却前にいくつかのフィルタを通ります。

- **共有サービス** は除外されます — それらはホスト上で動作し、Coast 内ではありません。
- **ワンショットジョブ**（ポートを持たないサービス）は、正常終了後は非表示になります。失敗した場合は、調査できるよう表示されます。
- **欠落サービス** — 存在すべき長時間稼働サービスが出力にない場合、問題が分かるよう `down` ステータスとして追加されます。

より深い調査には、`coast logs` でサービス出力を tail し、[`coast exec`](EXEC_AND_DOCKER.md) で Coast コンテナ内のシェルを取得してください。ログのストリーミングと MCP のトレードオフの詳細は [Logs](LOGS.md) を参照してください。

```bash
coast logs dev-1 --service web --tail 100
coast exec dev-1
```

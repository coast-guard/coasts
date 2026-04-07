# アーキテクチャ

リモート coast は、ローカルマシンとリモートサーバーの間で実行を分割します。デーモンがすべての操作を SSH トンネル経由で透過的にルーティングするため、開発者体験は変わりません。

## 2 コンテナ分割

すべてのリモート coast は 2 つのコンテナを作成します。

### Shell Coast (local)

あなたのマシン上の軽量な Docker コンテナです。通常の coast と同じバインドマウント（`/host-project`、`/workspace`）を持ちますが、内部 Docker デーモンも compose services もありません。エントリーポイントは `sleep infinity` です。

シェル coast が存在する理由は 1 つだけです。それは、ホスト側のエージェントやエディタが `/workspace` 配下のファイルを編集できるように、[filesystem bridge](../concepts_and_terminology/FILESYSTEM.md) を維持することです。これらの編集は [rsync and mutagen](FILE_SYNC.md) を介してリモートに同期されます。

### Remote Coast (remote)

リモートマシン上で `coast-service` によって管理されます。実際の作業が行われるのはここです。compose services を実行する完全な DinD コンテナであり、各サービスには動的ポートが割り当てられます。

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                            │
│                                                                          │
│  ┌────────────┐    unix     ┌───────────────────────────────────────┐    │
│  │ coast CLI  │───socket───▶│ coast-daemon                         │    │
│  └────────────┘             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shell Coast (sleep infinity)    │  │    │
│                             │  │ - /host-project (bind mount)    │  │    │
│                             │  │ - /workspace (mount --bind)     │  │    │
│                             │  │ - NO inner docker               │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Port Manager                    │  │    │
│                             │  │ - allocates local dynamic ports │  │    │
│                             │  │ - SSH -L tunnels to remote      │  │    │
│                             │  │   dynamic ports                 │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shared Services (local)         │  │    │
│                             │  │ - postgres, redis, etc.         │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  state.db (shadow instance,           │    │
│                             │           remote_host, port allocs)   │    │
│                             └───────────────────┬───────────────────┘    │
│                                                 │                        │
│                                    SSH tunnel   │  rsync / SSH           │
│                                                 │                        │
└─────────────────────────────────────────────────┼────────────────────────┘
                                                  │
┌─────────────────────────────────────────────────┼────────────────────────┐
│ REMOTE MACHINE                                  │                        │
│                                                 ▼                        │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │ coast-service (HTTP API on :31420)                                │   │
│  │                                                                   │   │
│  │  ┌───────────────────────────────────────────────────────────┐    │   │
│  │  │ DinD Container (per instance)                             │    │   │
│  │  │  /workspace (synced from local)                           │    │   │
│  │  │  compose services / bare services                         │    │   │
│  │  │  published on dynamic ports (e.g. :52340 -> :3000)        │    │   │
│  │  └───────────────────────────────────────────────────────────┘    │   │
│  │                                                                   │   │
│  │  Port Manager (dynamic port allocation per instance)              │   │
│  │  Build artifacts (/data/images/)                                  │   │
│  │  Image cache (/data/image-cache/)                                 │   │
│  │  Keystore (encrypted secrets)                                     │   │
│  │  remote-state.db (instances, worktrees)                           │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## SSH トンネル層

デーモンは 2 種類の SSH トンネルを使ってローカルとリモートを橋渡しします。

### Forward Tunnels (local to remote)

各サービスポートに対して、デーモンはローカルの動的ポートを対応するリモートの動的ポートにマッピングする `ssh -L` トンネルを作成します。これにより、`localhost:{dynamic_port}` がリモートサービスに到達できるようになります。

```text
ssh -N -L {local_dynamic}:localhost:{remote_dynamic} user@remote
```

`coast ports` を実行すると、dynamic 列にはこれらのローカルトンネルのエンドポイントが表示されます。

### Reverse Tunnels (remote to local)

[Shared services](../concepts_and_terminology/SHARED_SERVICES.md)（Postgres、Redis など）はあなたのローカルマシン上で動作します。デーモンは `ssh -R` トンネルを作成し、リモートの DinD コンテナがそれらに到達できるようにします。

```text
ssh -N -R 0.0.0.0:{remote_port}:localhost:{local_port} user@remote
```

リモート DinD コンテナ内では、サービスは `host.docker.internal:{port}` 経由で shared services に接続します。これは、リバーストンネルが待ち受けている Docker ブリッジゲートウェイに解決されます。

リモートホストの sshd では、リバーストンネルが `127.0.0.1` ではなく `0.0.0.0` にバインドできるようにするため、`GatewayPorts clientspecified` を有効にしておく必要があります。

### Tunnel Recovery

ラップトップがスリープしたりネットワークが変化したりすると、SSH トンネルは切断されることがあります。デーモンはバックグラウンドのヘルスループを実行し、以下を行います。

1. 5 秒ごとに TCP 接続で各動的ポートをプローブします。
2. あるインスタンスのすべてのポートが死んでいる場合、そのインスタンスの古いトンネルプロセスを kill して再確立します。
3. 一部のポートだけが死んでいる場合（部分障害）、正常なものを中断せずに不足しているトンネルだけを再確立します。
4. 新しいリバーストンネルを作成する前に、`fuser -k` で古いリモートポートバインディングをクリアします。

自己修復はインスタンス単位です。あるインスタンスのトンネルを復旧しても、別のインスタンスには影響しません。

## ポートフォワーディングチェーン

中間層のすべてのポートは動的です。正規ポートが存在するのはエンドポイントだけです。つまり、サービスが待ち受ける DinD コンテナ内と、[`coast checkout`](../concepts_and_terminology/CHECKOUT.md) を介したあなたの localhost 上です。

```text
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

この 3 ホップのチェーンにより、1 台のリモートマシン上で同じプロジェクトの複数インスタンスをポート競合なしで実行できます。各インスタンスは両側で独自の動的ポート集合を取得します。

## リクエストルーティング

すべてのデーモンハンドラは、インスタンス上の `remote_host` を確認します。設定されている場合、リクエストは SSH トンネル経由で coast-service に転送されます。

| Command | Remote behavior |
|---------|-----------------|
| `coast run` | ローカルで shell coast を作成 + artifact を転送 + coast-service に転送 |
| `coast build` | リモートマシン上でビルド（ローカルビルドの転送はしない） |
| `coast assign` | 新しい worktree の内容を rsync + assign リクエストを転送 |
| `coast exec` | coast-service に転送 |
| `coast ps` | coast-service に転送 |
| `coast logs` | coast-service に転送 |
| `coast stop` | 転送 + ローカル SSH トンネルを kill |
| `coast start` | 転送 + SSH トンネルを再確立 |
| `coast rm` | 転送 + トンネルを kill + ローカルの shadow instance を削除 |
| `coast checkout` | ローカルのみ（ホスト上の socat、転送不要） |
| `coast secret set` | ローカルに保存 + リモート keystore に転送 |

## coast-service

`coast-service` はリモートマシン上で動作するコントロールプレーンです。これはポート 31420 で待ち受ける HTTP サーバー（Axum）であり、build、run、assign、exec、ps、logs、stop、start、rm、secrets、および service restarts といったデーモンのローカル操作をミラーします。

これは独自の SQLite state database、Docker コンテナ（DinD）、動的ポート割り当て、build artifacts、image cache、そして暗号化された keystore を管理します。デーモンは SSH トンネル経由でのみこれと通信します。coast-service がパブリックインターネットに公開されることは決してありません。

デプロイ手順については [Setup](SETUP.md) を参照してください。

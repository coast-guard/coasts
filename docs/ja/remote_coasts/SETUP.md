# セットアップ

このページでは、リモート coast を実行するために必要なすべてを扱います: リモートホストの準備、coast-service のデプロイ、リモートの登録、そして最初のリモート coast の実行です。

## ホスト要件

| 要件 | 理由 |
|---|---|
| Docker | coast-service と DinD コンテナを実行するため |
| `GatewayPorts clientspecified` in sshd_config | [shared services](../concepts_and_terminology/SHARED_SERVICES.md) の SSH リバーストンネルが localhost のみに限定されず、すべてのインターフェースでバインドできるようにするため |
| SSH ユーザーに対するパスワードなし sudo | デーモンはワークスペースのファイル管理に `sudo rsync` と `sudo chown` を使用します（リモートワークスペースのディレクトリは coast-service の操作により root が所有している場合があります） |
| `/data` のバインドマウント（Docker ボリュームではない） | デーモンは SSH 経由でホストのファイルシステムにファイルを rsync します。名前付き Docker ボリュームはホストのファイルシステムから分離されており、rsync からは見えません |
| 50 GB 以上のディスク | Docker イメージはホスト Docker、tarball、そして DinD コンテナにロードされた状態で存在します。詳細は [disk management](CLI.md#disk-management) を参照してください |
| SSH アクセス | デーモンはトンネル、rsync、coast-service API へのアクセスのために SSH 経由でリモートに接続します |

## リモートホストを準備する

新しい Linux マシン上で（EC2、GCP、ベアメタル）:

```bash
# Install Docker and git
sudo yum install -y docker git          # Amazon Linux
# sudo apt-get install -y docker.io git # Ubuntu/Debian

# Enable Docker and add your user to the docker group
sudo systemctl enable docker
sudo systemctl start docker
sudo usermod -aG docker $(whoami)

# Enable GatewayPorts for shared service tunnels
sudo sh -c 'echo "GatewayPorts clientspecified" >> /etc/ssh/sshd_config'
sudo systemctl restart sshd

# Create the data directory with correct ownership
sudo mkdir -p /data && sudo chown $(whoami):$(whoami) /data
```

docker グループの変更を有効にするため、ログアウトして再度ログインしてください。

## coast-service をデプロイする

リポジトリをクローンし、本番用イメージをビルドします:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

バインドマウントを使って実行します（Docker ボリュームではありません）:

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

実行されていることを確認します:

```bash
curl http://localhost:31420/health
# ok
```

### なぜ `--privileged` が必要なのか

coast-service は Docker-in-Docker コンテナを管理します。`--privileged` フラグは、ネストされた Docker デーモンを実行するために必要な権限をコンテナに付与します。

### なぜ Docker ボリュームではなくバインドマウントなのか

デーモンは SSH 経由であなたのラップトップからリモートホストへワークスペースファイルを rsync します。これらのファイルはホストのファイルシステム上の `/data/workspaces/{project}/{instance}/` に配置されます。もし `/data` が名前付き Docker ボリュームであれば、ファイルは Docker のストレージ内に隔離され、コンテナ内で実行されている coast-service からは見えなくなります。

`-v /data:/data`（バインドマウント）を使用し、`-v coast-data:/data`（名前付きボリューム）は使用しないでください。

## リモートを登録する

ローカルマシン上で:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

カスタム SSH ポートを使用する場合:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

接続性をテストします:

```bash
coast remote test my-vm
```

これにより、SSH アクセスを確認し、SSH トンネル経由でポート 31420 上の coast-service に到達できることを確認し、リモートのアーキテクチャと coast-service のバージョンを報告します。

## ビルドと実行

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

この後は、すべての標準コマンドが動作します:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## 複数のリモート

複数のリモートマシンを登録できます:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

実行またはビルド時には、対象のリモートを指定します:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

登録されているリモートが 1 つだけの場合は、自動的に選択されます。

## ローカル開発セットアップ

coast-service 自体を開発するには、DinD、sshd、および cargo-watch によるホットリロードを含む開発用コンテナを使用します:

```bash
make coast-service-dev
```

その後、開発用コンテナをリモートとして登録します:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

`--key` フラグには絶対パスを使用してください。

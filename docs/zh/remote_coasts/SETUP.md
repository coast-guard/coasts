# 设置

本页涵盖让远程 coast 运行所需的一切:准备远程主机、部署 coast-service、注册远程端，以及运行你的第一个远程 coast。

## 主机要求

| 要求 | 原因 |
|---|---|
| Docker | 运行 coast-service 和 DinD 容器 |
| `GatewayPorts clientspecified` in sshd_config | 允许用于[共享服务](../concepts_and_terminology/SHARED_SERVICES.md)的 SSH 反向隧道绑定到所有网络接口，而不仅仅是 localhost |
| SSH 用户的免密码 sudo | 守护进程使用 `sudo rsync` 和 `sudo chown` 来管理工作区文件（由于 coast-service 操作，远程工作区目录可能归 root 所有） |
| `/data` 的绑定挂载（不是 Docker volume） | 守护进程通过 SSH 将文件 rsync 到主机文件系统。命名 Docker volume 与主机文件系统隔离，且 rsync 无法看到 |
| 50 GB+ 磁盘空间 | Docker 镜像存在于主机 Docker 中、tar 包中，并被加载到 DinD 容器中。详见[磁盘管理](CLI.md#disk-management) |
| SSH 访问 | 守护进程通过 SSH 连接到远程端，以进行隧道、rsync 和 coast-service API 访问 |

## 准备远程主机

在一台全新的 Linux 机器上（EC2、GCP、裸金属）:

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

注销并重新登录，以使 docker group 变更生效。

## 部署 coast-service

克隆仓库并构建生产镜像:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

使用绑定挂载运行它（不是 Docker volume）:

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

验证其正在运行:

```bash
curl http://localhost:31420/health
# ok
```

### 为什么使用 `--privileged`

coast-service 管理 Docker-in-Docker 容器。`--privileged` 标志授予容器运行嵌套 Docker 守护进程所需的能力。

### 为什么使用绑定挂载，而不是 Docker volume

守护进程通过 SSH 将工作区文件从你的笔记本电脑 rsync 到远程主机。这些文件会落到主机文件系统中的 `/data/workspaces/{project}/{instance}/`。如果 `/data` 是一个命名 Docker volume，这些文件将与 Docker 的存储隔离，并且对容器内运行的 coast-service 不可见。

使用 `-v /data:/data`（绑定挂载），不要使用 `-v coast-data:/data`（命名 volume）。

## 注册远程端

在你的本地机器上:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

使用自定义 SSH 端口:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

测试连通性:

```bash
coast remote test my-vm
```

这会验证 SSH 访问，检查 coast-service 是否可通过 SSH 隧道在 31420 端口访问，并报告远程端的架构和 coast-service 版本。

## 构建并运行

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

之后，所有标准命令都可正常使用:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## 多个远程端

你可以注册不止一台远程机器:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

在运行或构建时，指定要使用的远程端:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

如果只注册了一个远程端，则会自动选择它。

## 本地开发设置

为了开发 coast-service 本身，请使用开发容器，其中包含 DinD、sshd 和 cargo-watch 热重载:

```bash
make coast-service-dev
```

然后将开发容器注册为远程端:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

对 `--key` 标志请使用绝对路径。

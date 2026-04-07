# 架构

远程 coast 会将执行拆分到你的本地机器和远程服务器之间。开发者体验保持不变，因为守护进程会通过 SSH 隧道透明地路由每一个操作。

## 双容器拆分

每个远程 coast 都会创建两个容器:

### Shell Coast（本地）

运行在你机器上的一个轻量级 Docker 容器。它具有与普通 coast 相同的绑定挂载（`/host-project`、`/workspace`），但没有内部 Docker 守护进程，也没有 compose 服务。它的入口点是 `sleep infinity`。

shell coast 存在只有一个原因:它保留了 [文件系统桥接](../concepts_and_terminology/FILESYSTEM.md)，这样主机侧代理和编辑器就可以编辑 `/workspace` 下的文件。这些编辑会通过 [rsync 和 mutagen](FILE_SYNC.md) 同步到远程。

### Remote Coast（远程）

由远程机器上的 `coast-service` 管理。这是真正进行实际工作的地方:一个完整的 DinD 容器运行你的 compose 服务，并为每个服务分配动态端口。

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

## SSH 隧道层

守护进程使用两种 SSH 隧道来桥接本地和远程:

### 正向隧道（本地到远程）

对于每个服务端口，守护进程都会创建一个 `ssh -L` 隧道，将一个本地动态端口映射到对应的远程动态端口。这就是 `localhost:{dynamic_port}` 能够访问远程服务的原因。

```text
ssh -N -L {local_dynamic}:localhost:{remote_dynamic} user@remote
```

当你运行 `coast ports` 时，dynamic 列显示的就是这些本地隧道端点。

### 反向隧道（远程到本地）

[共享服务](../concepts_and_terminology/SHARED_SERVICES.md)（Postgres、Redis 等）运行在你的本地机器上。守护进程会创建 `ssh -R` 隧道，以便远程 DinD 容器可以访问它们:

```text
ssh -N -R 0.0.0.0:{remote_port}:localhost:{local_port} user@remote
```

在远程 DinD 容器内部，服务通过 `host.docker.internal:{port}` 连接到共享服务，它会解析到 Docker bridge 网关，而反向隧道正监听在那里。

远程主机的 sshd 必须启用 `GatewayPorts clientspecified`，这样反向隧道才能绑定到 `0.0.0.0`，而不是 `127.0.0.1`。

### 隧道恢复

当你的笔记本进入睡眠状态或网络发生变化时，SSH 隧道可能会中断。守护进程会运行一个后台健康检查循环，它会:

1. 每 5 秒通过 TCP 连接探测每个动态端口。
2. 如果某个实例的所有端口都失效，则杀掉该实例的陈旧隧道进程并重新建立它们。
3. 如果只有部分端口失效（部分故障），则仅重新建立缺失的隧道，而不会影响健康的隧道。
4. 在创建新的反向隧道之前，通过 `fuser -k` 清理陈旧的远程端口绑定。

恢复是按实例进行的——恢复某个实例的隧道绝不会中断另一个实例的隧道。

## 端口转发链

中间层中的所有端口都是动态的。规范端口只存在于端点:服务监听的 DinD 容器内部，以及通过 [`coast checkout`](../concepts_and_terminology/CHECKOUT.md) 暴露在你本机 localhost 上的端点。

```text
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

这个三跳链路允许同一项目的多个实例运行在同一台远程机器上而不会发生端口冲突。每个实例在两端都会获得自己的一组动态端口。

## 请求路由

每个守护进程处理器都会检查实例上的 `remote_host`。如果已设置，请求会通过 SSH 隧道转发到 coast-service:

| Command | Remote behavior |
|---------|-----------------|
| `coast run` | 在本地创建 shell coast + 传输构建产物 + 转发到 coast-service |
| `coast build` | 在远程机器上构建（不转发本地构建） |
| `coast assign` | Rsync 新的 worktree 内容 + 转发 assign 请求 |
| `coast exec` | 转发到 coast-service |
| `coast ps` | 转发到 coast-service |
| `coast logs` | 转发到 coast-service |
| `coast stop` | 转发 + 杀掉本地 SSH 隧道 |
| `coast start` | 转发 + 重新建立 SSH 隧道 |
| `coast rm` | 转发 + 杀掉隧道 + 删除本地影子实例 |
| `coast checkout` | 仅本地（主机上使用 socat，无需转发） |
| `coast secret set` | 本地存储 + 转发到远程 keystore |

## coast-service

`coast-service` 是运行在远程机器上的控制平面。它是一个监听 31420 端口的 HTTP 服务器（Axum），镜像了守护进程的本地操作:build、run、assign、exec、ps、logs、stop、start、rm、secrets 和服务重启。

它管理自己的 SQLite 状态数据库、Docker 容器（DinD）、动态端口分配、构建产物、镜像缓存以及加密 keystore。守护进程只通过 SSH 隧道与它通信——`coast-service` 永远不会暴露到公共互联网。

部署说明请参见 [Setup](SETUP.md)。

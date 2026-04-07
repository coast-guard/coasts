# 설정

이 페이지에서는 원격 coast를 실행하는 데 필요한 모든 것을 다룹니다: 원격 호스트 준비, coast-service 배포, 원격 등록, 그리고 첫 번째 원격 coast 실행.

## 호스트 요구 사항

| Requirement | Why |
|---|---|
| Docker | coast-service 및 DinD 컨테이너를 실행합니다 |
| `GatewayPorts clientspecified` in sshd_config | [shared services](../concepts_and_terminology/SHARED_SERVICES.md)를 위한 SSH reverse tunnel이 localhost뿐 아니라 모든 인터페이스에 바인딩되도록 허용합니다 |
| Passwordless sudo for SSH user | 데몬은 워크스페이스 파일 관리를 위해 `sudo rsync` 및 `sudo chown`을 사용합니다 (원격 워크스페이스 디렉터리는 coast-service 작업으로 인해 root 소유일 수 있습니다) |
| Bind mount for `/data` (not a Docker volume) | 데몬은 SSH를 통해 파일을 호스트 파일시스템으로 rsync합니다. 이름 있는 Docker 볼륨은 호스트 파일시스템과 격리되어 있으며 rsync에서 보이지 않습니다 |
| 50 GB+ disk | Docker 이미지는 호스트 Docker, tarball, 그리고 DinD 컨테이너에 로드된 형태로 존재합니다. 자세한 내용은 [disk management](CLI.md#disk-management)를 참조하세요 |
| SSH access | 데몬은 터널, rsync, 그리고 coast-service API 접근을 위해 SSH를 통해 원격에 연결합니다 |

## 원격 호스트 준비

새 Linux 머신(EC2, GCP, bare metal)에서:

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

docker 그룹 변경 사항이 적용되도록 로그아웃한 뒤 다시 로그인하세요.

## coast-service 배포

저장소를 클론하고 프로덕션 이미지를 빌드합니다:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

bind mount로 실행합니다(Docker 볼륨 아님):

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

실행 중인지 확인합니다:

```bash
curl http://localhost:31420/health
# ok
```

### Why `--privileged`

coast-service는 Docker-in-Docker 컨테이너를 관리합니다. `--privileged` 플래그는 중첩된 Docker 데몬을 실행하는 데 필요한 권한을 컨테이너에 부여합니다.

### Why bind mount, not Docker volume

데몬은 SSH를 통해 워크스페이스 파일을 노트북에서 원격 호스트로 rsync합니다. 이 파일들은 호스트 파일시스템의 `/data/workspaces/{project}/{instance}/`에 저장됩니다. 만약 `/data`가 이름 있는 Docker 볼륨이라면, 파일들은 Docker의 스토리지 내부에 격리되어 컨테이너 내부에서 실행 중인 coast-service에서 보이지 않게 됩니다.

`-v /data:/data` (bind mount)를 사용하고, `-v coast-data:/data` (named volume)는 사용하지 마세요.

## 원격 등록

로컬 머신에서:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

사용자 지정 SSH 포트를 사용하는 경우:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

연결을 테스트합니다:

```bash
coast remote test my-vm
```

이 명령은 SSH 접근을 확인하고, SSH 터널을 통해 31420 포트에서 coast-service에 접근 가능한지 검사하며, 원격의 아키텍처와 coast-service 버전을 보고합니다.

## 빌드 및 실행

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

이후에는 모든 표준 명령이 동작합니다:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## 여러 원격

하나 이상의 원격 머신을 등록할 수 있습니다:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

실행하거나 빌드할 때는 대상으로 삼을 원격을 지정합니다:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

등록된 원격이 하나뿐이면 자동으로 선택됩니다.

## 로컬 개발 설정

coast-service 자체를 개발하려면 DinD, sshd, 그리고 cargo-watch hot reload가 포함된 dev 컨테이너를 사용하세요:

```bash
make coast-service-dev
```

그다음 dev 컨테이너를 원격으로 등록합니다:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

`--key` 플래그에는 절대 경로를 사용하세요.

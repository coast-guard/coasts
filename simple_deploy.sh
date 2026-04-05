#!/bin/bash
# Deploy coastd-dev daemon to remote VM with all prerequisites
# Installs: Rust, Docker, Docker Compose, Mutagen

set -e

VM_HOST="ubuntu@192.168.122.139"
VM_PASSWORD="ubuntu"
BUILD_DIR="/home/ubuntu/coasts"

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo "=========================================="
echo "Remote VM Setup & coastd-dev Deployment"
echo "=========================================="
echo ""

# Step 1: Install Docker
echo -e "${BLUE}Step 1: Installing Docker...${NC}"
sshpass -p "$VM_PASSWORD" ssh -o StrictHostKeyChecking=no "$VM_HOST" '
    if command -v docker &> /dev/null; then
        echo "Docker already installed: $(docker --version)"
    else
        echo "Installing Docker..."
        # Install prerequisites
        sudo apt-get update -qq
        sudo apt-get install -y ca-certificates curl gnupg lsb-release

        # Add Docker GPG key
        sudo install -m 0755 -d /etc/apt/keyrings
        curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
        sudo chmod a+r /etc/apt/keyrings/docker.gpg

        # Add Docker repository
        echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

        # Install Docker
        sudo apt-get update -qq
        sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

        # Add user to docker group
        sudo usermod -aG docker $USER
        echo "Docker installed successfully!"
    fi
'
echo -e "${GREEN}✓ Docker ready${NC}"
echo ""

# Step 2: Install Mutagen
echo -e "${BLUE}Step 2: Installing Mutagen...${NC}"
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" '
    if command -v mutagen &> /dev/null; then
        echo "Mutagen already installed: $(mutagen version)"
    else
        echo "Installing Mutagen..."
        MUTAGEN_VERSION="0.18.1"
        
        # Download and extract
        cd /tmp
        curl -sL "https://github.com/mutagen-io/mutagen/releases/download/v${MUTAGEN_VERSION}/mutagen_linux_amd64_v${MUTAGEN_VERSION}.tar.gz" | tar xz
        
        # Install binary
        mkdir -p ~/.local/bin
        mv mutagen ~/.local/bin/
        chmod +x ~/.local/bin/mutagen
        
        # Install agent bundles
        mkdir -p ~/.local/libexec
        tar xzf mutagen-agents.tar.gz -C ~/.local/libexec
        
        # Gzip the agents (Mutagen expects .gz files)
        cd ~/.local/libexec
        for f in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64; do
            if [ -f "$f" ] && [ ! -f "$f.gz" ]; then
                gzip -k "$f" 2>/dev/null || true
            fi
        done
        
        # Add to PATH if not already there
        if ! grep -q "/.local/bin" ~/.bashrc; then
            echo "export PATH=\$HOME/.local/bin:\$PATH" >> ~/.bashrc
        fi
        
        echo "Mutagen installed: $(~/.local/bin/mutagen version)"
    fi
'
echo -e "${GREEN}✓ Mutagen ready${NC}"
echo ""

# Step 3: Install Rust and build tools
echo -e "${BLUE}Step 3: Installing Rust and build tools...${NC}"
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "
    # Install Rust
    if ! command -v cargo &> /dev/null; then
        echo 'Installing Rust...'
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    else
        echo 'Rust already installed'
    fi
    
    # Install build-essential
    if ! command -v cc &> /dev/null; then
        echo 'Installing build tools...'
        sudo apt-get update -qq
        sudo apt-get install -y build-essential pkg-config libssl-dev
    fi
"
echo -e "${GREEN}✓ Build tools ready${NC}"
echo ""

# Step 4: Upload source code
echo -e "${BLUE}Step 4: Uploading source code...${NC}"
echo "Creating source archive (excluding large directories)..."
tar czf /tmp/coasts-source.tar.gz \
    --exclude='target' \
    --exclude='.git' \
    --exclude='node_modules' \
    --exclude='log.txt' \
    --exclude='*.log' \
    --exclude='build_output*.txt' \
    .

sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "rm -rf $BUILD_DIR && mkdir -p $BUILD_DIR"
sshpass -p "$VM_PASSWORD" scp /tmp/coasts-source.tar.gz "$VM_HOST:$BUILD_DIR/"
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "cd $BUILD_DIR && tar xzf coasts-source.tar.gz && rm coasts-source.tar.gz"
rm /tmp/coasts-source.tar.gz

echo -e "${GREEN}✓ Source uploaded${NC}"
echo ""

# Step 5: Build coastd-dev
echo -e "${BLUE}Step 5: Building coastd-dev (daemon only)...${NC}"
echo "This will take 5-10 minutes for first build..."
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "
    source \$HOME/.cargo/env
    cd $BUILD_DIR
    
    # Build only the daemon (skip UI)
    echo 'Building coastd-dev...'
    cargo build --bin coastd-dev
    
    # Install to ~/.local/bin
    mkdir -p ~/.local/bin
    cp target/debug/coastd-dev ~/.local/bin/coastd-dev
    chmod +x ~/.local/bin/coastd-dev
    
    echo 'Build complete!'
    ~/.local/bin/coastd-dev --help | head -n 3
" 2>&1 | tee vm_build.log

if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo -e "${RED}✗ Build failed${NC}"
    echo "Check vm_build.log for details"
    exit 1
fi

echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Step 6: Start daemon
echo -e "${BLUE}Step 6: Starting daemon on VM...${NC}"
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "pkill -f coastd-dev || true"
sleep 1
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "
    source \$HOME/.cargo/env
    export PATH=\$HOME/.local/bin:\$PATH
    nohup ~/.local/bin/coastd-dev > /tmp/coastd-dev.log 2>&1 &
"
sleep 2

if sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "pgrep -f coastd-dev" > /dev/null; then
    PID=$(sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "pgrep -f coastd-dev")
    echo -e "${GREEN}✓ Daemon started (PID: $PID)${NC}"
else
    echo -e "${RED}✗ Failed to start daemon${NC}"
    echo "Checking logs:"
    sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "cat /tmp/coastd-dev.log"
    exit 1
fi
echo ""

# Step 7: Create workspace directories
echo -e "${BLUE}Step 7: Creating workspace directories...${NC}"
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "
    mkdir -p ~/remote-workspace
    mkdir -p ~/coast-workspaces
"
echo -e "${GREEN}✓ Workspace directories created${NC}"
echo ""

# Summary
echo -e "${GREEN}=========================================="
echo "Setup Complete!"
echo "==========================================${NC}"
echo ""
echo -e "${YELLOW}Installed on remote VM:${NC}"
sshpass -p "$VM_PASSWORD" ssh "$VM_HOST" "
    echo \"  - Docker: \$(docker --version 2>/dev/null || echo 'not found')\"
    echo \"  - Docker Compose: \$(docker compose version 2>/dev/null || echo 'not found')\"
    echo \"  - Mutagen: \$(~/.local/bin/mutagen version 2>/dev/null || echo 'not found')\"
    echo \"  - Rust: \$(~/.cargo/bin/rustc --version 2>/dev/null || echo 'not found')\"
    echo "  - coastd-dev: $(test -x ~/.local/bin/coastd-dev && echo 'installed' || echo 'not found')"
"
echo ""
echo "Remote daemon is running on: $VM_HOST"
echo ""
echo -e "${BLUE}Next Steps:${NC}"
cat << 'EOF'
# 1. Ensure local daemon is running
coastd-dev &

# 2. Add remote (if not already added)
coast-dev remote add ubuntu-vm ubuntu@192.168.122.139 \
    --workspace-root /home/ubuntu/remote-workspace \
    -i ~/.ssh/coast_vm_key

# 3. Connect to remote
coast-dev remote connect ubuntu-vm

# 4. Create sync session for your project
cd /path/to/your/project
coast-dev sync create <project-name> --remote ubuntu-vm --path $(pwd)

# 5. Run your app on the remote VM
ssh ubuntu-vm "cd ~/coast-workspaces/<project>/main && docker compose up -d"
EOF
echo ""

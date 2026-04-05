#!/bin/bash
# Install Mutagen on Linux

set -e

echo "Installing Mutagen..."

# Download latest version
MUTAGEN_VERSION="0.17.6"
curl -L "https://github.com/mutagen-io/mutagen/releases/download/v${MUTAGEN_VERSION}/mutagen_linux_amd64_v${MUTAGEN_VERSION}.tar.gz" -o /tmp/mutagen.tar.gz

# Extract
cd /tmp
tar -xzf mutagen.tar.gz

# Install to user bin
mkdir -p ~/.local/bin
mv mutagen ~/.local/bin/
chmod +x ~/.local/bin/mutagen

# Add to PATH if not already
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
    export PATH="$HOME/.local/bin:$PATH"
fi

# Start mutagen daemon
~/.local/bin/mutagen daemon start

# Verify
~/.local/bin/mutagen version

echo ""
echo "✓ Mutagen installed successfully!"
echo "  Location: ~/.local/bin/mutagen"
echo "  Version: $MUTAGEN_VERSION"
echo ""
echo "Add to PATH in current shell:"
echo '  export PATH="$HOME/.local/bin:$PATH"'

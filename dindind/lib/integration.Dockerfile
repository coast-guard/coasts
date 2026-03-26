# Integration test image: base DinD + Rust toolchain + sed compat shim.
# Used by integration-runner.sh to build coast binaries inside the container.
FROM coast-dindind-base

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    rsync \
    docker-buildx-plugin \
  && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable \
  && /root/.cargo/bin/rustup component add clippy rustfmt

ENV PATH="/root/.cargo/bin:${PATH}"

# BSD sed -i '' compatibility: override sed so setup.sh works on GNU/Linux
COPY lib/sed-compat.sh /usr/local/bin/sed
RUN chmod +x /usr/local/bin/sed

# Shared base image for DinDinD integration test scenarios.
# Provides Ubuntu + Docker CE + Coast dependencies.
#
# Build args:
#   DISTRO          -- base image (default: ubuntu:22.04)
#   EXTRA_PACKAGES  -- space-separated list of additional apt packages
ARG DISTRO=ubuntu:22.04
FROM ${DISTRO}

ARG EXTRA_PACKAGES=""
ARG DEBIAN_FRONTEND=noninteractive

# Disable universe/multiverse repos to avoid slow/unreliable ports.ubuntu.com downloads.
RUN sed -i '/universe/d; /multiverse/d' /etc/apt/sources.list \
  && apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    gnupg \
    lsb-release \
    bash \
    git \
    socat \
    sudo \
    iptables \
    openssh-server \
    psmisc \
    rsync \
    ${EXTRA_PACKAGES} \
  && rm -rf /var/lib/apt/lists/*

# Install Docker CE from the official repository
RUN install -m 0755 -d /etc/apt/keyrings \
  && curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
     | gpg --dearmor -o /etc/apt/keyrings/docker.gpg \
  && chmod a+r /etc/apt/keyrings/docker.gpg \
  && echo \
       "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
       https://download.docker.com/linux/ubuntu \
       $(lsb_release -cs) stable" \
     > /etc/apt/sources.list.d/docker.list \
  && apt-get update \
  && apt-get install -y --no-install-recommends \
       docker-ce \
       docker-ce-cli \
       containerd.io \
       docker-compose-plugin \
  && rm -rf /var/lib/apt/lists/*

# Install Node.js (LTS)
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
  && apt-get install -y --no-install-recommends nodejs \
  && rm -rf /var/lib/apt/lists/*

# Install mutagen for continuous file sync in remote coast tests.
# Must extract both the mutagen binary AND the agents bundle.
RUN curl -fsSL https://github.com/mutagen-io/mutagen/releases/download/v0.18.1/mutagen_linux_amd64_v0.18.1.tar.gz \
    | tar xz -C /usr/local/bin \
  && chmod +x /usr/local/bin/mutagen

# Non-root user matching typical WSL/Linux desktop setup
RUN useradd -m -s /bin/bash testuser \
  && usermod -aG docker testuser \
  && echo "testuser ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

COPY lib/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

VOLUME /var/lib/docker
ENTRYPOINT ["entrypoint.sh"]

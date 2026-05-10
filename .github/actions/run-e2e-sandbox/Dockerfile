FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

ARG NODE_VERSION=26

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    git \
    jq \
    python3 \
    python-is-python3 \
    xz-utils \
  && rm -rf /var/lib/apt/lists/* \
  && ARCH=$(uname -m | sed 's/x86_64/x64/' | sed 's/aarch64/arm64/') \
  && curl -fsSL "https://nodejs.org/dist/latest-v${NODE_VERSION}.x/SHASUMS256.txt" -o /tmp/SHASUMS256.txt \
  && NODE_FILENAME=$(grep "linux-${ARCH}.tar.xz" /tmp/SHASUMS256.txt | awk '{print $2}') \
  && curl -fsSL "https://nodejs.org/dist/latest-v${NODE_VERSION}.x/${NODE_FILENAME}" -o /tmp/node.tar.xz \
  && echo "$(grep "${NODE_FILENAME}" /tmp/SHASUMS256.txt | awk '{print $1}')  /tmp/node.tar.xz" | sha256sum -c - \
  && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
  && rm -f /tmp/node.tar.xz /tmp/SHASUMS256.txt \
  && npm install -g pnpm

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]

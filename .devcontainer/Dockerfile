# Base image
ARG VARIANT=bookworm-slim
FROM debian:${VARIANT}
ENV DEBIAN_FRONTEND=noninteractive
ENV LC_ALL=C.UTF-8
ENV LANG=C.UTF-8
ENV ESPFLASH_PORT=/dev/ttyACM0

# Arguments
ARG CONTAINER_USER=esp
ARG CONTAINER_GROUP=esp
ARG ESP_BOARD=esp32c3
ARG GITHUB_TOKEN

# Install system dependencies + Docker CE CLI
# curl and ca-certificates are bootstrapped first to add the Docker apt repository
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && printf "deb [arch=%s signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian %s stable\n" \
        "$(dpkg --print-architecture)" "$(. /etc/os-release && echo "${VERSION_CODENAME}")" \
        > /etc/apt/sources.list.d/docker.list \
    && apt-get update \
    && apt-get install -y \
        clang \
        cmake \
        curl \
        docker-ce-cli \
        git \
        groff \
        libclang-dev \
        libssl-dev \
        libudev-dev \
        libusb-1.0-0 \
        llvm-dev \
        make \
        mandoc \
        ninja-build \
        pkg-config \
        python3 \
        python3-venv \
        unzip \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

# Install uv (system-wide, architecture-aware via the official installer)
RUN curl -fsSL https://astral.sh/uv/install.sh | UV_INSTALL_DIR=/usr/local/bin sh

# Install AWS CLI v2 (architecture-aware)
RUN DPKG_ARCH="$(dpkg --print-architecture)" \
    && case "$DPKG_ARCH" in \
        amd64) AWS_ARCH="x86_64"  ;; \
        arm64) AWS_ARCH="aarch64" ;; \
        *) printf "Unsupported architecture: %s\n" "$DPKG_ARCH" >&2 && exit 1 ;; \
    esac \
    && curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-${AWS_ARCH}.zip" -o /tmp/awscliv2.zip \
    && unzip /tmp/awscliv2.zip -d /tmp/awscli \
    && /tmp/awscli/aws/install \
    && rm -rf /tmp/awscliv2.zip /tmp/awscli

# Install AWS SAM CLI (architecture-aware)
RUN DPKG_ARCH="$(dpkg --print-architecture)" \
    && case "$DPKG_ARCH" in \
        amd64) SAM_ARCH="x86_64" ;; \
        arm64) SAM_ARCH="arm64"  ;; \
        *) printf "Unsupported architecture: %s\n" "$DPKG_ARCH" >&2 && exit 1 ;; \
    esac \
    && curl -fsSL "https://github.com/aws/aws-sam-cli/releases/latest/download/aws-sam-cli-linux-${SAM_ARCH}.zip" -o /tmp/aws-sam-cli.zip \
    && unzip /tmp/aws-sam-cli.zip -d /tmp/sam-installation \
    && /tmp/sam-installation/install \
    && rm -rf /tmp/aws-sam-cli.zip /tmp/sam-installation

# Create user with serial port access
RUN adduser --disabled-password --gecos "" ${CONTAINER_USER} \
    && usermod -aG dialout ${CONTAINER_USER}

USER ${CONTAINER_USER}
WORKDIR /home/${CONTAINER_USER}

# Install rustup (toolchain version controlled by rust-toolchain.toml)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
    --default-toolchain none -y --profile minimal

ENV PATH="${PATH}:/home/${CONTAINER_USER}/.cargo/bin"

# Install esp-rs tooling (espup, espflash, cargo-espflash, web-flash)
# Uses the rustup host triple to select the correct pre-built binary
RUN RUST_ARCH="$($HOME/.cargo/bin/rustup show | grep 'Default host' | sed 's/.* //')" \
    \
    && curl -fsSL "https://github.com/esp-rs/espup/releases/latest/download/espup-${RUST_ARCH}" \
        -o "${HOME}/.cargo/bin/espup" \
    && chmod u+x "${HOME}/.cargo/bin/espup" \
    \
    && curl -fsSL "https://github.com/esp-rs/espflash/releases/latest/download/cargo-espflash-${RUST_ARCH}.zip" \
        -o /tmp/cargo-espflash.zip \
    && unzip /tmp/cargo-espflash.zip -d "${HOME}/.cargo/bin/" \
    && chmod u+x "${HOME}/.cargo/bin/cargo-espflash" \
    && rm /tmp/cargo-espflash.zip \
    \
    && curl -fsSL "https://github.com/esp-rs/espflash/releases/latest/download/espflash-${RUST_ARCH}.zip" \
        -o /tmp/espflash.zip \
    && unzip /tmp/espflash.zip -d "${HOME}/.cargo/bin/" \
    && chmod u+x "${HOME}/.cargo/bin/espflash" \
    && rm /tmp/espflash.zip \
    \
    && curl -fsSL "https://github.com/esp-rs/esp-web-flash-server/releases/latest/download/web-flash-${RUST_ARCH}.zip" \
        -o /tmp/web-flash.zip \
    && unzip /tmp/web-flash.zip -d "${HOME}/.cargo/bin/" \
    && chmod u+x "${HOME}/.cargo/bin/web-flash" \
    && rm /tmp/web-flash.zip

# Install ESP toolchain via espup (handles RISC-V for ESP32C3)
RUN if [ -n "${GITHUB_TOKEN}" ]; then export GITHUB_TOKEN="${GITHUB_TOKEN}"; fi \
    && "${HOME}/.cargo/bin/espup" install \
        --targets "${ESP_BOARD}" \
        --log-level info \
        --export-file "/home/${CONTAINER_USER}/export-esp.sh"

# Activate ESP environment on login
RUN echo "source /home/${CONTAINER_USER}/export-esp.sh" >> ~/.bashrc

CMD ["/bin/bash"]

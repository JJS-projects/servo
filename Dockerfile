FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    TERM=xterm-256color \
    CARGO_HOME=/root/.cargo \
    RUSTUP_HOME=/root/.rustup \
    PATH=/root/.cargo/bin:/root/.local/bin:${PATH} \
    RUSTC_WRAPPER=sccache \
    SCCACHE_DIR=/root/.cache/sccache \
    SCCACHE_CACHE_SIZE=20G

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    ccache \
    clang \
    cmake \
    curl \
    g++ \
    git \
    gperf \
    libdbus-1-dev \
    libegl1-mesa-dev \
    libfreetype6-dev \
    libges-1.0-dev \
    libgl1-mesa-dri \
    libgles2-mesa-dev \
    libglib2.0-dev \
    libgstreamer-plugins-bad1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-good1.0-dev \
    libgstrtspserver-1.0-dev \
    libgstreamer1.0-dev \
    libharfbuzz-dev \
    liblzma-dev \
    libudev-dev \
    libunwind-dev \
    libvulkan1 \
    libx11-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-x11-0 \
    libxkbcommon0 \
    libxmu-dev \
    libxmu6 \
    llvm-dev \
    m4 \
    pkg-config \
    python3 \
    python3-dev \
    python3-pip \
    python3-toml \
    python3-venv \
    sccache \
    xorg-dev \
    xz-utils \
    gstreamer1.0-libav \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-tools && \
    rm -rf /var/lib/apt/lists/*

RUN curl -LsSf https://astral.sh/uv/install.sh | sh
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal

COPY python/requirements.txt /tmp/requirements.txt

RUN git clone --depth 1 https://github.com/servo/servo.git /opt/servo
WORKDIR /opt/servo

RUN python3 -m pip install --break-system-packages -r /tmp/requirements.txt && \
    python3 ./mach bootstrap --yes && \
    python3 ./mach build --dev

WORKDIR /workspace/servo-terminal
CMD ["/bin/bash"]

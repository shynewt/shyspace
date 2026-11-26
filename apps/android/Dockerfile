# Everything the Android app needs to build, so the host needs no JDK, Android SDK, NDK or
# Android Rust targets. The Rust version must match rust-toolchain.toml (the Makefile passes it).
# Pinned by digest (the multi-arch index of 21-jdk as of 2026-10-29), so a rebuild gets the same base.
FROM docker.io/library/eclipse-temurin:21-jdk@sha256:ebfbb18d11ff163ee83b21258acf12248af48d22a0fdfdc68fa1a1516c05976a

# perl and make build the vendored OpenSSL under SQLCipher; build-essential builds the core
# for the host, which the JVM tests load.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential curl perl unzip && \
    rm -rf /var/lib/apt/lists/*

ENV ANDROID_HOME=/opt/android-sdk
ENV PATH="${ANDROID_HOME}/cmdline-tools/latest/bin:${ANDROID_HOME}/platform-tools:${PATH}"
ARG NDK_VERSION=29.0.14206865
ENV ANDROID_NDK_HOME=${ANDROID_HOME}/ndk/${NDK_VERSION}

# The zip's SHA-1 matches Google's repository2-3.xml; its SHA-256 is pinned here.
ARG CMDLINE_TOOLS_SHA256=0877a1d048fe4a24efe2eff536ca4223f7adeb58648bb81909d33c446918cfa8
RUN mkdir -p ${ANDROID_HOME}/cmdline-tools && \
    curl -sSfL https://dl.google.com/android/repository/commandlinetools-linux-16111833_latest.zip -o /tmp/cmdline-tools.zip && \
    echo "${CMDLINE_TOOLS_SHA256}  /tmp/cmdline-tools.zip" | sha256sum -c - && \
    unzip -q /tmp/cmdline-tools.zip -d /tmp/cmdline-tools && \
    mv /tmp/cmdline-tools/cmdline-tools ${ANDROID_HOME}/cmdline-tools/latest && \
    rm -rf /tmp/cmdline-tools.zip /tmp/cmdline-tools && \
    (yes | sdkmanager --licenses > /dev/null) && \
    sdkmanager "platforms;android-37.2" "build-tools;37.0.0" "platform-tools" "ndk;${NDK_VERSION}" > /dev/null

ARG RUST_VERSION
# rustup-init itself, pinned with the checksum rustup publishes beside it (x86_64 build machines).
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_INIT_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
ENV RUSTUP_HOME=/opt/rustup
ENV PATH="/opt/cargo/bin:${PATH}"
RUN test -n "${RUST_VERSION}" && \
    curl -sSfL https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init -o /tmp/rustup-init && \
    echo "${RUSTUP_INIT_SHA256}  /tmp/rustup-init" | sha256sum -c - && \
    chmod +x /tmp/rustup-init && \
    CARGO_HOME=/opt/cargo /tmp/rustup-init -y --no-modify-path --profile minimal \
        --default-toolchain ${RUST_VERSION} --component rustfmt --component clippy \
        --target aarch64-linux-android --target x86_64-linux-android && \
    rm /tmp/rustup-init && \
    CARGO_HOME=/opt/cargo cargo install --locked cargo-ndk --version 4.1.2 && \
    rm -rf /opt/cargo/registry /opt/cargo/git
